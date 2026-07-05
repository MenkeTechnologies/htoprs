//! Port of `pcp/Platform.c` + `.h` — the PCP platform backend's global state
//! and platform hooks. This is the last file of htop's Performance Co-Pilot
//! backend.
//!
//! 1:1 faithful port; the C is the spec. `Platform` is htop's own struct (not a
//! libpcp type), so it is modeled idiomatically (owned `Vec`s for the C
//! `xCalloc`'d `pmID*`/`pmDesc*` arrays, owned `PCPDynamic*` tables) rather than
//! by C layout. The `pcp` global (`Platform.c:57`) is an [`AtomicPtr`] to a
//! leaked `Box` (the C `xCalloc`'d global lives for the program lifetime); every
//! `Platform_*` function loads and dereferences it exactly as the C assumes
//! non-null (a null deref is the faithful "not initialized" crash). Flexible /
//! union access is under `unsafe`, mirroring the [`Metric`] layer.
//!
//! # CLI-options substrate (deferred)
//!
//! `Platform_init`, `Platform_getLongOption`, and `Platform_longOptionsUsage`
//! are ported as honest `todo!()` bodies: `Platform_init` and
//! `Platform_getLongOption` need the `pmOptions` global (`opts`) plus
//! `pmGetOptions`/`pmGetContextOptions`/`optind`/`optarg`/`__pmAddOptHost` CLI
//! substrate — a large, version-dependent (`PMAPI_VERSION`) bitfield struct
//! whose exact layout is unsafe to transcribe by hand. Rather than risk a wrong
//! `pmOptions` layout (worse than a scaffolded stub), the `opts` global is not
//! declared and the CLI-options trio is deferred as a group.
//! `Platform_longOptionsUsage` is opts-free itself but is grouped with its
//! siblings (the CLI path is otherwise unported, so printing partial help is
//! pointless). Everything else in `Platform.c` is ported.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(dead_code)]

use core::any::Any;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_uint};
use std::ptr;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::Mutex;

use crate::ported::action::Htop_Action;
use crate::ported::batterymeter::ACPresence;
use crate::ported::commandline::CommandLineStatus;
use crate::ported::crt::ColorElements;
use crate::ported::diskiometer::DiskIOData;
use crate::ported::hashtable::{Hashtable, Hashtable_get};
use crate::ported::machine::Machine;
use crate::ported::meter::Meter;
use crate::ported::networkiometer::NetworkIOData;
use crate::ported::panel::Panel;
use crate::ported::processlocksscreen::FileLocks_ProcessData;
use crate::ported::richstring::RichString;
use crate::ported::settings::{ScreenSettings, Settings};
use crate::ported::xutils::{sumPositiveValues, String_eq};
use crate::ported::zfsarcmeter::ZfsArcMeter_readStats;
use crate::ported::zfscompressedarcmeter::ZfsCompressedArcMeter_readStats;

use crate::ported::linux::linuxmachine::ZramStats;

use crate::ported::pcp::metric::{
    Metric, Metric_enable, Metric_fromId, Metric_instance, Metric_instanceCount, Metric_values,
};
use crate::ported::pcp::pcpdynamiccolumn::{
    PCPDynamicColumn, PCPDynamicColumn_writeField, PCPDynamicColumns, PCPDynamicColumns_done,
    PCPDynamicColumns_setupWidths,
};
use crate::ported::pcp::pcpdynamicmeter::{
    PCPDynamicMeter, PCPDynamicMeter_display, PCPDynamicMeter_enable, PCPDynamicMeter_updateValues,
    PCPDynamicMeters, PCPDynamicMeters_done,
};
use crate::ported::pcp::pcpdynamicscreen::{
    PCPDynamicScreen_addDynamicScreen, PCPDynamicScreen_appendScreens,
    PCPDynamicScreen_appendTables, PCPDynamicScreens, PCPDynamicScreens_addAvailableColumns,
    PCPDynamicScreens_done,
};
use crate::ported::pcp::pcpmachine::{
    PCPMachine, SystemName, CPU_FREQUENCY, CPU_GUEST_PERIOD, CPU_IOWAIT_PERIOD, CPU_IRQ_PERIOD,
    CPU_NICE_PERIOD, CPU_SOFTIRQ_PERIOD, CPU_STEAL_PERIOD, CPU_SYSTEM_ALL_PERIOD,
    CPU_SYSTEM_PERIOD, CPU_TOTAL_PERIOD, CPU_USER_PERIOD, MEMORY_CLASS_ACTIVE,
    MEMORY_CLASS_AVAILABLE, MEMORY_CLASS_BUFFERS, MEMORY_CLASS_CACHE, MEMORY_CLASS_COMPRESSED,
    MEMORY_CLASS_INACTIVE, MEMORY_CLASS_LIMIT, MEMORY_CLASS_PURGEABLE, MEMORY_CLASS_SHARED,
    MEMORY_CLASS_SPECULATIVE, MEMORY_CLASS_USED, MEMORY_CLASS_WIRED,
};
use crate::ported::pcp::pcpprocess::PCPProcess;
use crate::ported::pcp::pmapi::{
    pmAtomValue, pmDesc, pmDestroyContext, pmFreeResult, pmGetContextHostName, pmID, pmLookupDesc,
    pmResult, pmtimevalDec, PM_ID_NULL, PM_TYPE_32, PM_TYPE_64, PM_TYPE_DOUBLE, PM_TYPE_STRING,
    PM_TYPE_U32, PM_TYPE_U64,
};

use Metric::*;

/// Port of `typedef struct Platform_` (`pcp/Platform.h:45`) — the PCP backend's
/// global state. The C `xCalloc`'d `names`/`pmids`/`fetch`/`descs` arrays
/// (indexed by [`Metric`]) are owned `Vec`s; `result` is the libpcp-owned
/// `pmFetch` output (a raw pointer, freed via `pmFreeResult`). The
/// `PCPDynamic*` tables and the archive/uname tail fields (`offset`/`btime`/
/// `release`/`pidmax`/`ncpu`) are the deferred fields the `Platform_*` functions
/// use.
pub struct Platform {
    /// C `int context` — the PMAPI(3) context identifier.
    pub context: c_int,
    /// C `bool reconnect` — the context needs reconnecting.
    pub reconnect: bool,
    /// C `size_t totalMetrics` — total number of all metrics.
    pub totalMetrics: usize,
    /// C `const char** names` — metric name array indexed by `Metric`.
    pub names: Vec<*const c_char>,
    /// C `pmID* pmids` — all known metric identifiers, indexed by `Metric`.
    pub pmids: Vec<pmID>,
    /// C `pmID* fetch` — enabled identifiers for sampling (`PM_ID_NULL` = off).
    pub fetch: Vec<pmID>,
    /// C `pmDesc* descs` — metric descriptor array indexed by `Metric`.
    pub descs: Vec<pmDesc>,
    /// C `pmResult* result` — the latest `pmFetch` sample values (libpcp-owned).
    pub result: *mut pmResult,
    /// C `PCPDynamicMeters meters` — dynamic meters via configuration files.
    pub meters: PCPDynamicMeters,
    /// C `PCPDynamicColumns columns` — dynamic columns via configuration files.
    pub columns: PCPDynamicColumns,
    /// C `PCPDynamicScreens screens` — dynamic screens via configuration files.
    pub screens: PCPDynamicScreens,
    /// C `struct timeval offset` — time offset used in archive mode only.
    pub offset: libc::timeval,
    /// C `long long btime` — boottime in seconds since the epoch.
    pub btime: i64,
    /// C `char* release` — uname and distro from this context.
    pub release: Option<String>,
    /// C `int pidmax` — maximum platform process identifier.
    pub pidmax: c_int,
    /// C `unsigned int ncpu` — maximum processor count configured.
    pub ncpu: c_uint,
}

/// Port of `Platform* pcp` (`pcp/Platform.c:57`) — the single global PCP backend
/// state, set up by `Platform_init` (a leaked `Box`, as the C `xCalloc`'d global
/// lives for the program's lifetime). Null until initialized; [`Metric`] and the
/// `Platform_*` functions load and dereference it. Modeled as an `AtomicPtr`
/// (the `CRT_*` global pattern).
pub static pcp: AtomicPtr<Platform> = AtomicPtr::new(ptr::null_mut());

// CPUMeter.h `CPU_METER_*` indices into `Meter::values` (the shuffled htop
// CPU-time buckets), matching the `linux/Platform.c` port's local consts.
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

// SwapMeter.h `SWAP_METER_*` indices into `Meter::values`.
const SWAP_METER_USED: usize = 0;
const SWAP_METER_CACHE: usize = 1;
const SWAP_METER_FRONTSWAP: usize = 2;

/// Port of `typedef struct MemoryClass_` (`MemoryMeter.h`) — one memory-meter
/// class: its label, whether it counts toward the "used"/"cache" totals, and its
/// `CRT_colors` element.
pub struct MemoryClass {
    pub label: &'static str,
    pub countsAsUsed: bool,
    pub countsAsCache: bool,
    pub color: ColorElements,
}

/// Port of `static const MemoryClass Linux_memoryClasses[]` (`Platform.c:80`).
/// Written in `MEMORY_CLASS_*` index order (0..5); the C uses designated
/// initializers, and the labels are deliberately shuffled relative to the slot
/// names (that is the C, faithfully).
const Linux_memoryClasses: [MemoryClass; MEMORY_CLASS_LIMIT] = [
    // [MEMORY_CLASS_USED]
    MemoryClass {
        label: "used",
        countsAsUsed: true,
        countsAsCache: false,
        color: ColorElements::MEMORY_1,
    },
    // [MEMORY_CLASS_SHARED]
    MemoryClass {
        label: "shared",
        countsAsUsed: true,
        countsAsCache: false,
        color: ColorElements::MEMORY_2,
    },
    // [MEMORY_CLASS_BUFFERS]
    MemoryClass {
        label: "compressed",
        countsAsUsed: true,
        countsAsCache: false,
        color: ColorElements::MEMORY_3,
    },
    // [MEMORY_CLASS_CACHE]
    MemoryClass {
        label: "buffers",
        countsAsUsed: false,
        countsAsCache: false,
        color: ColorElements::MEMORY_4,
    },
    // [MEMORY_CLASS_COMPRESSED]
    MemoryClass {
        label: "cache",
        countsAsUsed: false,
        countsAsCache: false,
        color: ColorElements::MEMORY_5,
    },
    // [MEMORY_CLASS_AVAILABLE]
    MemoryClass {
        label: "available",
        countsAsUsed: false,
        countsAsCache: false,
        color: ColorElements::MEMORY_6,
    },
];

/// Port of `static const MemoryClass Darwin_memoryClasses[]` (`Platform.c:89`).
/// Written in `MEMORY_CLASS_*` index order (0..5).
const Darwin_memoryClasses: [MemoryClass; MEMORY_CLASS_LIMIT] = [
    // [MEMORY_CLASS_WIRED]
    MemoryClass {
        label: "wired",
        countsAsUsed: true,
        countsAsCache: false,
        color: ColorElements::MEMORY_1,
    },
    // [MEMORY_CLASS_SPECULATIVE]
    MemoryClass {
        label: "speculative",
        countsAsUsed: true,
        countsAsCache: true,
        color: ColorElements::MEMORY_2,
    },
    // [MEMORY_CLASS_ACTIVE]
    MemoryClass {
        label: "active",
        countsAsUsed: true,
        countsAsCache: false,
        color: ColorElements::MEMORY_3,
    },
    // [MEMORY_CLASS_PURGEABLE]
    MemoryClass {
        label: "purgeable",
        countsAsUsed: false,
        countsAsCache: true,
        color: ColorElements::MEMORY_4,
    },
    // [MEMORY_CLASS_COMPRESSED]
    MemoryClass {
        label: "compressed",
        countsAsUsed: true,
        countsAsCache: false,
        color: ColorElements::MEMORY_5,
    },
    // [MEMORY_CLASS_INACTIVE]
    MemoryClass {
        label: "inactive",
        countsAsUsed: true,
        countsAsCache: true,
        color: ColorElements::MEMORY_6,
    },
];

/// Port of `MemoryClass Platform_memoryClasses[MEMORY_CLASS_LIMIT]`
/// (`Platform.c:98`) — the dynamically-adjusted memory model, selected between
/// the Linux and Darwin sets by [`Platform_setRelease`] via the C
/// `memcpy(Platform_memoryClasses, ...)`. Modeled as a [`Mutex`]-guarded array
/// (the sound analog of the C global-array mutation); seeded with the Linux set
/// (the C zero-init is overwritten on the first `Platform_setRelease`).
static Platform_memoryClasses: Mutex<[MemoryClass; MEMORY_CLASS_LIMIT]> =
    Mutex::new(Linux_memoryClasses);

/// Port of `int pmLookupDescs(int numpmid, pmID* pmids, pmDesc* descs)`
/// (`Platform.c:305`, the `#ifndef HAVE_PMLOOKUPDESCS` htop fallback). Looks up
/// each enabled metric's descriptor via [`pmLookupDesc`], disabling
/// (`PM_ID_NULL`) any that fail, and returns the count of successful lookups.
/// The C `pmDebugOptions.appl0` error-logging branch (`pcp->names[i]`,
/// [`pmIDStr`](super::pmapi::pmIDStr), `pmErrStr`) is debug chatter, omitted.
pub fn pmLookupDescs(numpmid: c_int, pmids: *mut pmID, descs: *mut pmDesc) -> c_int {
    unsafe {
        let mut count = 0;
        for i in 0..numpmid {
            let idx = i as usize;
            // expect some metrics to be missing - e.g. PMDA not available
            if *pmids.add(idx) == PM_ID_NULL {
                continue;
            }
            let sts = pmLookupDesc(*pmids.add(idx), descs.add(idx));
            if sts < 0 {
                // C logs via pmDebugOptions.appl0 here — debug chatter, omitted.
                *pmids.add(idx) = PM_ID_NULL;
                continue;
            }
            count += 1;
        }
        count
    }
}

/// Port of `size_t Platform_addMetric(Metric id, const char* name)`
/// (`Platform.c:328`). Registers a metric name into the `pcp->names`/`pmids`/
/// `fetch`/`descs` registry (growing the arrays by one for a
/// configuration-file metric beyond `PCP_METRIC_COUNT`), disables it for the
/// initial sample (`PM_ID_NULL`), and returns the new total metric count.
///
/// Deviation: the C stores the caller-owned `const char*` pointer directly
/// (the static `Platform_metricNames` table for the built-in metrics, or a
/// column/meter's owned `metricName` string for config metrics). Rust cannot
/// soundly alias a `&str`'s bytes, so a NUL-terminated copy is leaked
/// ([`CString::into_raw`]) to give `pcp->names[i]` the stable `'static` pointer
/// `pmLookupName` requires — a small one-time leak matching the C names' program
/// lifetime.
pub fn Platform_addMetric(id: Metric, name: &str) -> usize {
    unsafe {
        let p = &mut *pcp.load(Ordering::Relaxed);
        let i = id as usize;

        if i >= PCP_METRIC_COUNT as usize && i >= p.totalMetrics {
            // added via configuration files — xRealloc the arrays by one.
            let j = p.totalMetrics + 1;
            p.fetch.resize(j, PM_ID_NULL);
            p.pmids.resize(j, PM_ID_NULL);
            p.names.resize(j, ptr::null());
            // memset(&pcp->descs[i], 0, sizeof(pmDesc)) — the new slot is zeroed.
            p.descs.resize_with(j, || core::mem::zeroed());
        }

        // Leak a stable NUL-terminated copy (see the doc-comment deviation note).
        let cname = CString::new(name)
            .expect("Platform_addMetric: name has interior NUL")
            .into_raw();
        p.pmids[i] = PM_ID_NULL;
        p.fetch[i] = PM_ID_NULL;
        p.names[i] = cname as *const c_char;
        p.totalMetrics += 1;
        p.totalMetrics
    }
}

/// TODO: port of `bool Platform_init(void)` (`Platform.c:349`). Deferred: needs
/// the `pmOptions` global (`opts`) + `pmGetOptions`/`pmGetContextOptions`/
/// `pmNewContext` CLI substrate (option (b) — the `pmOptions` layout is too
/// version-dependent to transcribe safely). See the module doc.
pub fn Platform_init() -> bool {
    todo!("pcp/Platform.c:349 Platform_init — deferred (needs the pmOptions/pmGetOptions CLI substrate)")
}

/// Port of `void Platform_dynamicColumnsDone(Hashtable* columns)`
/// (`Platform.c:461`).
pub fn Platform_dynamicColumnsDone(columns: &Hashtable) {
    PCPDynamicColumns_done(columns);
}

/// Port of `void Platform_dynamicMetersDone(Hashtable* meters)`
/// (`Platform.c:465`).
pub fn Platform_dynamicMetersDone(meters: &Hashtable) {
    PCPDynamicMeters_done(meters);
}

/// Port of `void Platform_dynamicScreensDone(Hashtable* screens)`
/// (`Platform.c:469`).
pub fn Platform_dynamicScreensDone(screens: &Hashtable) {
    PCPDynamicScreens_done(screens);
}

/// Port of `void Platform_done(void)` (`Platform.c:473`). Destroys the PMAPI
/// context, frees the latest sample result, then reclaims the leaked `Box`
/// (whose `Drop` frees the `Vec`s and `release` string — the C `free`s of
/// `release`/`fetch`/`pmids`/`names`/`descs`/`pcp`). The leaked metric-name
/// CStrings are not freed, matching the C freeing only the names array.
pub fn Platform_done() {
    unsafe {
        let raw = pcp.swap(ptr::null_mut(), Ordering::Relaxed);
        if raw.is_null() {
            return;
        }
        let b = Box::from_raw(raw);
        pmDestroyContext(b.context);
        if !b.result.is_null() {
            pmFreeResult(b.result);
        }
        // free(release/fetch/pmids/names/descs/pcp) — Box `Drop` frees the rest.
    }
}

/// Port of `void Platform_setBindings(Htop_Action* keys)` (`Platform.c:485`) —
/// no platform-specific key bindings (`(void)keys`).
pub fn Platform_setBindings(keys: &mut [Option<Htop_Action>]) {
    // no platform-specific key bindings
    let _ = keys;
}

/// Port of `int Platform_getUptime(void)` (`Platform.c:490`).
pub fn Platform_getUptime() -> c_int {
    unsafe {
        let mut value: pmAtomValue = core::mem::zeroed();
        if Metric_values(PCP_UPTIME, &mut value, 1, PM_TYPE_32).is_null() {
            return 0;
        }
        value.l
    }
}

/// Port of `void Platform_getLoadAverage(double* one, double* five, double*
/// fifteen)` (`Platform.c:497`).
pub fn Platform_getLoadAverage(one: &mut f64, five: &mut f64, fifteen: &mut f64) {
    *one = 0.0;
    *five = 0.0;
    *fifteen = 0.0;

    unsafe {
        let mut values: [pmAtomValue; 3] = [core::mem::zeroed(); 3];
        if !Metric_values(PCP_LOAD_AVERAGE, values.as_mut_ptr(), 3, PM_TYPE_DOUBLE).is_null() {
            *one = values[0].d;
            *five = values[1].d;
            *fifteen = values[2].d;
        }
    }
}

/// Port of `unsigned int Platform_getMaxCPU(void)` (`Platform.c:508`). Caches
/// the processor count into `pcp->ncpu` (defaulting to `1` on a fetch miss).
pub fn Platform_getMaxCPU() -> u32 {
    unsafe {
        let p = &mut *pcp.load(Ordering::Relaxed);
        if p.ncpu != 0 {
            return p.ncpu;
        }

        let mut value: pmAtomValue = core::mem::zeroed();
        if !Metric_values(PCP_HINV_NCPU, &mut value, 1, PM_TYPE_U32).is_null() {
            p.ncpu = value.ul;
        } else {
            p.ncpu = 1;
        }
        p.ncpu
    }
}

/// Port of `pid_t Platform_getMaxPid(void)` (`Platform.c:520`). Caches the
/// maximum pid into `pcp->pidmax`, falling back to `INT_MAX` on a fetch miss.
pub fn Platform_getMaxPid() -> libc::pid_t {
    unsafe {
        let p = &mut *pcp.load(Ordering::Relaxed);
        if p.pidmax != 0 {
            return p.pidmax as libc::pid_t;
        }

        let mut value: pmAtomValue = core::mem::zeroed();
        if Metric_values(PCP_PID_MAX, &mut value, 1, PM_TYPE_32).is_null() {
            return c_int::MAX as libc::pid_t;
        }
        p.pidmax = value.l;
        p.pidmax as libc::pid_t
    }
}

/// Port of `long long Platform_getBootTime(void)` (`Platform.c:531`). Caches the
/// boot time (seconds since the epoch) into `pcp->btime`.
pub fn Platform_getBootTime() -> libc::c_longlong {
    unsafe {
        let p = &mut *pcp.load(Ordering::Relaxed);
        if p.btime != 0 {
            return p.btime;
        }

        let mut value: pmAtomValue = core::mem::zeroed();
        if !Metric_values(PCP_BOOTTIME, &mut value, 1, PM_TYPE_64).is_null() {
            p.btime = value.ll;
        }
        p.btime
    }
}

/// Port of `static double Platform_setOneCPUValues(Meter* this, const Settings*
/// settings, pmAtomValue* values)` (`Platform.c:541`). Fills the shuffled htop
/// CPU-time percentages (`this->values[CPU_METER_*]`) from one CPU's sampled
/// PERIOD buckets (`values[CPU_*_PERIOD]`), honoring `detailedCPUTime` (8-class
/// breakdown vs 4-class summary) and `accountGuestInCPUMeter`, and returns the
/// summed active percentage (capped at 100). `values` is the aggregate `cpu` or
/// one `percpu[]` buffer (indexed by [`CPUMetric`](super::pcpmachine)).
fn Platform_setOneCPUValues(this: &mut Meter, settings: &Settings, values: &[pmAtomValue]) -> f64 {
    unsafe {
        let mut value = values[CPU_TOTAL_PERIOD].ull;
        let total = (if value == 0 { 1 } else { value }) as f64;

        this.values[CPU_METER_NICE] = values[CPU_NICE_PERIOD].ull as f64 / total * 100.0;
        this.values[CPU_METER_NORMAL] = values[CPU_USER_PERIOD].ull as f64 / total * 100.0;
        if settings.detailedCPUTime {
            this.values[CPU_METER_KERNEL] = values[CPU_SYSTEM_PERIOD].ull as f64 / total * 100.0;
            this.values[CPU_METER_IRQ] = values[CPU_IRQ_PERIOD].ull as f64 / total * 100.0;
            this.values[CPU_METER_SOFTIRQ] = values[CPU_SOFTIRQ_PERIOD].ull as f64 / total * 100.0;
            this.curItems = 5;

            this.values[CPU_METER_STEAL] = values[CPU_STEAL_PERIOD].ull as f64 / total * 100.0;
            this.values[CPU_METER_GUEST] = values[CPU_GUEST_PERIOD].ull as f64 / total * 100.0;
            if settings.accountGuestInCPUMeter {
                this.curItems = 7;
            }

            this.values[CPU_METER_IOWAIT] = values[CPU_IOWAIT_PERIOD].ull as f64 / total * 100.0;
        } else {
            this.values[CPU_METER_KERNEL] =
                values[CPU_SYSTEM_ALL_PERIOD].ull as f64 / total * 100.0;
            value = values[CPU_STEAL_PERIOD].ull + values[CPU_GUEST_PERIOD].ull;
            this.values[CPU_METER_IRQ] = value as f64 / total * 100.0;
            this.curItems = 4;
        }

        let mut percent = sumPositiveValues(&this.values[..this.curItems as usize]);
        percent = percent.min(100.0);

        if settings.detailedCPUTime {
            this.curItems = 8;
        }

        this.values[CPU_METER_FREQUENCY] = values[CPU_FREQUENCY].d;
        this.values[CPU_METER_TEMPERATURE] = f64::NAN;

        percent
    }
}

/// Port of `double Platform_setCPUValues(Meter* this, int cpu)`
/// (`Platform.c:582`). Uses the aggregate `cpu` buffer for `cpu <= 0`, else the
/// `percpu[cpu - 1]` buffer. `this->host` is the concrete [`PCPMachine`]; the
/// settings live on its base [`Machine`].
pub fn Platform_setCPUValues(this: &mut Meter, cpu: c_int) -> f64 {
    unsafe {
        let phost = &*(this.host as *const PCPMachine);
        let settings = (*this.host)
            .settings
            .as_ref()
            .expect("Platform_setCPUValues: host->settings (C dereferences it)");

        if cpu <= 0 {
            // use aggregate values
            Platform_setOneCPUValues(this, settings, phost.cpu.as_slice())
        } else {
            Platform_setOneCPUValues(this, settings, phost.percpu[(cpu - 1) as usize].as_slice())
        }
    }
}

/// Port of `static void Platform_setLinuxMemoryValues(double* v, const
/// PCPMachine* host)` (`Platform.c:591`). Copies the procps-style Linux memory
/// classes from `host->memValue[]`, then applies the ZFS-ARC shrinkable and
/// zswap-compression adjustments.
fn Platform_setLinuxMemoryValues(v: &mut [f64], host: &PCPMachine) {
    v[MEMORY_CLASS_USED] = host.memValue[MEMORY_CLASS_USED] as f64;
    v[MEMORY_CLASS_SHARED] = host.memValue[MEMORY_CLASS_SHARED] as f64;
    v[MEMORY_CLASS_BUFFERS] = host.memValue[MEMORY_CLASS_BUFFERS] as f64;
    v[MEMORY_CLASS_CACHE] = host.memValue[MEMORY_CLASS_CACHE] as f64;
    v[MEMORY_CLASS_AVAILABLE] = host.memValue[MEMORY_CLASS_AVAILABLE] as f64;

    if host.zfs.enabled != 0 {
        // ZFS does not shrink below the value of zfs_arc_min.
        let mut shrinkable_size: u64 = 0;
        if host.zfs.size > host.zfs.min {
            shrinkable_size = host.zfs.size - host.zfs.min;
        }
        v[MEMORY_CLASS_USED] -= shrinkable_size as f64;
        v[MEMORY_CLASS_CACHE] += shrinkable_size as f64;
        v[MEMORY_CLASS_AVAILABLE] += shrinkable_size as f64;
    }

    if host.zswap.usedZswapOrig > 0 || host.zswap.usedZswapComp > 0 {
        v[MEMORY_CLASS_USED] -= host.zswap.usedZswapComp as f64;
        v[MEMORY_CLASS_COMPRESSED] = host.zswap.usedZswapComp as f64;
    } else {
        v[MEMORY_CLASS_COMPRESSED] = 0.0;
    }
}

/// Port of `static void Platform_setDarwinMemoryValues(double* v, const
/// PCPMachine* host)` (`Platform.c:616`). Copies the Darwin memory classes from
/// `host->memValue[]`.
fn Platform_setDarwinMemoryValues(v: &mut [f64], host: &PCPMachine) {
    v[MEMORY_CLASS_WIRED] = host.memValue[MEMORY_CLASS_WIRED] as f64;
    v[MEMORY_CLASS_SPECULATIVE] = host.memValue[MEMORY_CLASS_SPECULATIVE] as f64;
    v[MEMORY_CLASS_ACTIVE] = host.memValue[MEMORY_CLASS_ACTIVE] as f64;
    v[MEMORY_CLASS_PURGEABLE] = host.memValue[MEMORY_CLASS_PURGEABLE] as f64;
    v[MEMORY_CLASS_COMPRESSED] = host.memValue[MEMORY_CLASS_COMPRESSED] as f64;
    v[MEMORY_CLASS_INACTIVE] = host.memValue[MEMORY_CLASS_INACTIVE] as f64;
}

/// Port of `void Platform_setMemoryValues(Meter* this)` (`Platform.c:625`).
/// Sets `total` from the host's `totalMem` and dispatches to the Linux/Darwin
/// memory-class fill (zeroing the six value slots for an unknown OS — the C
/// `memset(this->values, 0, sizeof(phost->memValue))`).
pub fn Platform_setMemoryValues(this: &mut Meter) {
    unsafe {
        let host = &*this.host;
        let phost = &*(this.host as *const PCPMachine);

        this.total = host.totalMem as f64;
        if phost.sys == SystemName::SYSTEM_NAME_LINUX {
            Platform_setLinuxMemoryValues(&mut this.values, phost);
        } else if phost.sys == SystemName::SYSTEM_NAME_DARWIN {
            Platform_setDarwinMemoryValues(&mut this.values, phost);
        } else {
            // memset(this->values, 0, sizeof(phost->memValue)) — 6 doubles.
            for i in 0..MEMORY_CLASS_LIMIT {
                this.values[i] = 0.0;
            }
        }
    }
}

/// Port of `void Platform_setSwapValues(Meter* this)` (`Platform.c:638`). Fills
/// the swap meter's `total`/`values` from the host's swap counters, then applies
/// the zswap adjustment (zswapped pages moved out of `USED` into `FRONTSWAP`,
/// overflow spilling into `CACHE`).
pub fn Platform_setSwapValues(this: &mut Meter) {
    unsafe {
        let host = &*this.host;
        let phost = &*(this.host as *const PCPMachine);

        this.total = host.totalSwap as f64;
        this.values[SWAP_METER_USED] = host.usedSwap as f64;
        this.values[SWAP_METER_CACHE] = host.cachedSwap as f64;
        this.values[SWAP_METER_FRONTSWAP] = 0.0; // frontswap accounted to swap but elsewhere

        if phost.zswap.usedZswapOrig > 0 || phost.zswap.usedZswapComp > 0 {
            // refer to linux/Platform.c::Platform_setSwapValues for details
            this.values[SWAP_METER_USED] -= phost.zswap.usedZswapOrig as f64;
            if this.values[SWAP_METER_USED] < 0.0 {
                // subtract the overflow from SwapCached
                this.values[SWAP_METER_CACHE] += this.values[SWAP_METER_USED];
                this.values[SWAP_METER_USED] = 0.0;
            }
            this.values[SWAP_METER_FRONTSWAP] += phost.zswap.usedZswapOrig as f64;
        }
    }
}

/// Port of `void Platform_setZramValues(Meter* this)` (`Platform.c:659`). Sums
/// the per-device zram capacity / original / compressed instances into the meter
/// (`total` = capacity, `values[0]` = compressed, `values[1]` = the extra
/// original bytes), clamping `usedZramComp <= usedZramOrig`.
pub fn Platform_setZramValues(this: &mut Meter) {
    unsafe {
        let count = Metric_instanceCount(PCP_ZRAM_CAPACITY);
        if count < 1 {
            this.total = 0.0;
            this.values[0] = 0.0;
            this.values[1] = 0.0;
            return;
        }

        let mut values: Vec<pmAtomValue> = vec![core::mem::zeroed(); count as usize];
        let mut stats = ZramStats::default();

        if !Metric_values(PCP_ZRAM_CAPACITY, values.as_mut_ptr(), count, PM_TYPE_U64).is_null() {
            for v in values.iter().take(count as usize) {
                stats.totalZram += v.ull;
            }
        }
        if !Metric_values(PCP_ZRAM_ORIGINAL, values.as_mut_ptr(), count, PM_TYPE_U64).is_null() {
            for v in values.iter().take(count as usize) {
                stats.usedZramOrig += v.ull;
            }
        }
        if !Metric_values(PCP_ZRAM_COMPRESSED, values.as_mut_ptr(), count, PM_TYPE_U64).is_null() {
            for v in values.iter().take(count as usize) {
                stats.usedZramComp += v.ull;
            }
        }

        // free(values) — the Vec drops at scope end.

        if stats.usedZramComp > stats.usedZramOrig {
            stats.usedZramComp = stats.usedZramOrig;
        }

        this.total = stats.totalZram as f64;
        this.values[0] = stats.usedZramComp as f64;
        this.values[1] = (stats.usedZramOrig - stats.usedZramComp) as f64;
    }
}

/// Port of `void Platform_setZfsArcValues(Meter* this)` (`Platform.c:695`).
pub fn Platform_setZfsArcValues(this: &mut Meter) {
    let phost = unsafe { &*(this.host as *const PCPMachine) };
    ZfsArcMeter_readStats(this, &phost.zfs);
}

/// Port of `void Platform_setZfsCompressedArcValues(Meter* this)`
/// (`Platform.c:701`).
pub fn Platform_setZfsCompressedArcValues(this: &mut Meter) {
    let phost = unsafe { &*(this.host as *const PCPMachine) };
    ZfsCompressedArcMeter_readStats(this, &phost.zfs);
}

/// Port of `void Platform_getHostname(char* buffer, size_t size)`
/// (`Platform.c:707`). Reads the context's hostname via [`pmGetContextHostName`].
/// The C `String_safeStrncpy` into a caller buffer is modeled as the returned
/// owned `String` (the `linux/Platform.c` `Platform_getHostname` idiom; the
/// `size` truncation is a caller concern the `String` elides).
pub fn Platform_getHostname() -> String {
    unsafe {
        let p = &*pcp.load(Ordering::Relaxed);
        let hostname = pmGetContextHostName(p.context);
        if hostname.is_null() {
            String::new()
        } else {
            CStr::from_ptr(hostname).to_string_lossy().into_owned()
        }
    }
}

/// Port of `static void Platform_setRelease(void)` (`Platform.c:712`). Reads the
/// uname/distro metrics, selects the global memory model
/// ([`Platform_memoryClasses`] ← Linux or Darwin set, keyed by `sysname`), and
/// builds `pcp->release` (`"sysname release [machine] @ distro "`).
///
/// The C's `/* cull trailing space */ pcp->release[strlen(pcp->release)] = '\0'`
/// is a no-op (it writes `'\0'` at the existing terminator), so the built string
/// retains its trailing space — faithfully reproduced here. The libpcp-malloc'd
/// metric strings are copied into the Rust `String` then `free`d.
fn Platform_setRelease() {
    unsafe {
        let p = &mut *pcp.load(Ordering::Relaxed);

        let mut sysname: pmAtomValue = core::mem::zeroed();
        let mut release: pmAtomValue = core::mem::zeroed();
        let mut machine: pmAtomValue = core::mem::zeroed();
        let mut distro: pmAtomValue = core::mem::zeroed();
        if Metric_values(PCP_UNAME_SYSNAME, &mut sysname, 1, PM_TYPE_STRING).is_null() {
            sysname.cp = ptr::null_mut();
        }
        if Metric_values(PCP_UNAME_RELEASE, &mut release, 1, PM_TYPE_STRING).is_null() {
            release.cp = ptr::null_mut();
        }
        if Metric_values(PCP_UNAME_MACHINE, &mut machine, 1, PM_TYPE_STRING).is_null() {
            machine.cp = ptr::null_mut();
        }
        if Metric_values(PCP_UNAME_DISTRO, &mut distro, 1, PM_TYPE_STRING).is_null() {
            distro.cp = ptr::null_mut();
        }

        // set global memory class model using sysname
        let is_darwin = !sysname.cp.is_null() && CStr::from_ptr(sysname.cp).to_bytes() == b"Darwin";
        if is_darwin {
            *Platform_memoryClasses.lock().unwrap() = Darwin_memoryClasses;
        } else {
            // default to the Linux memory categories
            *Platform_memoryClasses.lock().unwrap() = Linux_memoryClasses;
        }

        let mut s = String::new();
        if !sysname.cp.is_null() {
            s.push_str(&CStr::from_ptr(sysname.cp).to_string_lossy());
            s.push(' ');
        }
        if !release.cp.is_null() {
            s.push_str(&CStr::from_ptr(release.cp).to_string_lossy());
            s.push(' ');
        }
        if !machine.cp.is_null() {
            s.push('[');
            s.push_str(&CStr::from_ptr(machine.cp).to_string_lossy());
            s.push_str("] ");
        }
        if !distro.cp.is_null() {
            if !s.is_empty() {
                s.push_str("@ ");
                s.push_str(&CStr::from_ptr(distro.cp).to_string_lossy());
            } else {
                s.push_str(&CStr::from_ptr(distro.cp).to_string_lossy());
            }
            s.push(' ');
        }
        // C "cull trailing space" is a no-op — the trailing space is retained.
        p.release = Some(s);

        libc::free(distro.cp as *mut libc::c_void);
        libc::free(machine.cp as *mut libc::c_void);
        libc::free(release.cp as *mut libc::c_void);
        libc::free(sysname.cp as *mut libc::c_void);
    }
}

/// Port of `const char* Platform_getRelease(void)` (`Platform.c:772`). Builds
/// `pcp->release` on first use ([`Platform_setRelease`]) and returns it. The
/// returned `&str` borrows the leaked-`Box` global (program lifetime), hence
/// `'static`.
pub fn Platform_getRelease() -> Option<&'static str> {
    unsafe {
        let raw = pcp.load(Ordering::Relaxed);
        if (*raw).release.is_none() {
            Platform_setRelease();
        }
        (*raw).release.as_deref()
    }
}

/// Port of `char* Platform_getProcessEnv(pid_t pid)` (`Platform.c:779`). Fetches
/// the process's environment metric (`proc.psinfo.environ`) as a string. The C
/// `char*` result / `NULL` maps to `Option<String>` (the libpcp-malloc'd value
/// is copied into the `String` then `free`d).
pub fn Platform_getProcessEnv(pid: libc::pid_t) -> Option<String> {
    unsafe {
        let mut value: pmAtomValue = core::mem::zeroed();
        if Metric_instance(
            PCP_PROC_ENVIRON,
            pid as c_int,
            0,
            &mut value,
            PM_TYPE_STRING,
        )
        .is_null()
        {
            return None;
        }
        let s = CStr::from_ptr(value.cp).to_string_lossy().into_owned();
        libc::free(value.cp as *mut libc::c_void);
        Some(s)
    }
}

/// Port of `FileLocks_ProcessData* Platform_getProcessLocks(pid_t pid)`
/// (`Platform.c:786`) — PCP exposes no per-process file locks (`return NULL`).
pub fn Platform_getProcessLocks(pid: libc::pid_t) -> Option<FileLocks_ProcessData> {
    let _ = pid;
    None
}

/// Port of `void Platform_getPressureStall(const char* file, bool some, double*
/// ten, double* sixty, double* threehundred)` (`Platform.c:791`). Selects the
/// PSI metric for `file`/`some` and reads its 10/60/300-second averages.
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

    let metric = if String_eq(file, "cpu") {
        PCP_PSI_CPUSOME
    } else if String_eq(file, "io") {
        if some {
            PCP_PSI_IOSOME
        } else {
            PCP_PSI_IOFULL
        }
    } else if String_eq(file, "irq") {
        PCP_PSI_IRQFULL
    } else if String_eq(file, "mem") {
        if some {
            PCP_PSI_MEMSOME
        } else {
            PCP_PSI_MEMFULL
        }
    } else {
        return;
    };

    unsafe {
        let mut values: [pmAtomValue; 3] = [core::mem::zeroed(); 3];
        if !Metric_values(metric, values.as_mut_ptr(), 3, PM_TYPE_DOUBLE).is_null() {
            *ten = values[0].d;
            *sixty = values[1].d;
            *threehundred = values[2].d;
        }
    }
}

/// Port of `bool Platform_getDiskIO(DiskIOData* data)` (`Platform.c:814`).
pub fn Platform_getDiskIO(data: &mut DiskIOData) -> bool {
    // memset(data, 0, sizeof(*data));
    data.totalBytesRead = 0;
    data.totalBytesWritten = 0;
    data.totalMsTimeSpend = 0;
    data.numDisks = 0;

    unsafe {
        let mut value: pmAtomValue = core::mem::zeroed();
        if !Metric_values(PCP_DISK_READB, &mut value, 1, PM_TYPE_U64).is_null() {
            data.totalBytesRead = value.ull;
        }
        if !Metric_values(PCP_DISK_WRITEB, &mut value, 1, PM_TYPE_U64).is_null() {
            data.totalBytesWritten = value.ull;
        }
        if !Metric_values(PCP_DISK_ACTIVE, &mut value, 1, PM_TYPE_U64).is_null() {
            data.totalMsTimeSpend = value.ull;
        }
        if !Metric_values(PCP_HINV_NDISK, &mut value, 1, PM_TYPE_U64).is_null() {
            data.numDisks = value.ull;
        }
    }
    true
}

/// Port of `bool Platform_getNetworkIO(NetworkIOData* data)` (`Platform.c:829`).
/// (No `memset` — only the fetched fields are written, faithfully.)
pub fn Platform_getNetworkIO(data: &mut NetworkIOData) -> bool {
    unsafe {
        let mut value: pmAtomValue = core::mem::zeroed();
        if !Metric_values(PCP_NET_RECVB, &mut value, 1, PM_TYPE_U64).is_null() {
            data.bytesReceived = value.ull;
        }
        if !Metric_values(PCP_NET_SENDB, &mut value, 1, PM_TYPE_U64).is_null() {
            data.bytesTransmitted = value.ull;
        }
        if !Metric_values(PCP_NET_RECVP, &mut value, 1, PM_TYPE_U64).is_null() {
            data.packetsReceived = value.ull;
        }
        if !Metric_values(PCP_NET_SENDP, &mut value, 1, PM_TYPE_U64).is_null() {
            data.packetsTransmitted = value.ull;
        }
    }
    true
}

/// Port of `void Platform_getFileDescriptors(double* used, double* max)`
/// (`Platform.c:842`).
pub fn Platform_getFileDescriptors(used: &mut f64, max: &mut f64) {
    *used = f64::NAN;
    *max = 65536.0;

    unsafe {
        let mut value: pmAtomValue = core::mem::zeroed();
        if !Metric_values(PCP_VFS_FILES_COUNT, &mut value, 1, PM_TYPE_32).is_null() {
            *used = value.l as f64;
        }
        if !Metric_values(PCP_VFS_FILES_MAX, &mut value, 1, PM_TYPE_32).is_null() {
            *max = value.l as f64;
        }
    }
}

/// Port of `void Platform_getBattery(double* level, ACPresence* isOnAC)`
/// (`Platform.c:853`) — PCP has no battery metrics.
pub fn Platform_getBattery(level: &mut f64, isOnAC: &mut ACPresence) {
    *level = f64::NAN;
    *isOnAC = ACPresence::AC_ERROR;
}

/// Port of `const char* Platform_getFailedState(void)` (`Platform.c:858`).
pub fn Platform_getFailedState() -> Option<&'static str> {
    unsafe {
        let p = &*pcp.load(Ordering::Relaxed);
        if p.reconnect {
            Some("PMCD DOWN")
        } else {
            None
        }
    }
}

/// TODO: port of `void Platform_longOptionsUsage(const char* name)`
/// (`Platform.c:862`). Deferred as part of the CLI-options trio (it is opts-free
/// itself, but the CLI path is otherwise unported — see the module doc).
pub fn Platform_longOptionsUsage(name: &str) {
    let _ = name;
    todo!("pcp/Platform.c:862 Platform_longOptionsUsage — deferred with the CLI-options trio")
}

/// TODO: port of `CommandLineStatus Platform_getLongOption(int opt, int argc,
/// char** argv)` (`Platform.c:869`). Deferred: needs the `pmOptions` global
/// (`opts`) + `optind`/`optarg`/`__pmAddOptHost`/`pmprintf` CLI substrate (see
/// the module doc).
pub fn Platform_getLongOption(
    opt: c_int,
    argc: c_int,
    argv: *mut *mut c_char,
) -> CommandLineStatus {
    let _ = (opt, argc, argv);
    todo!("pcp/Platform.c:869 Platform_getLongOption — deferred (needs the pmOptions/opts CLI substrate)")
}

/// Port of `void Platform_gettime_realtime(struct timeval* tv, uint64_t* msec)`
/// (`Platform.c:907`). Reads wall-clock time, shifted by the archive-mode
/// `pcp->offset` to stay in lock-step with realtime.
pub fn Platform_gettime_realtime(tv: &mut libc::timeval, msec: &mut u64) {
    unsafe {
        if libc::gettimeofday(tv, ptr::null_mut()) == 0 {
            let p = &*pcp.load(Ordering::Relaxed);
            // shift by start offset to stay in lock-step with realtime (archives)
            if p.offset.tv_sec != 0 || p.offset.tv_usec != 0 {
                pmtimevalDec(tv, &p.offset);
            }
            *msec = (tv.tv_sec as u64 * 1000) + (tv.tv_usec as u64 / 1000);
        } else {
            *tv = core::mem::zeroed();
            *msec = 0;
        }
    }
}

/// Port of `void Platform_gettime_monotonic(uint64_t* msec)` (`Platform.c:919`).
/// Uses the latest sample's timestamp (the `PMAPI_VERSION >= 3` `timespec`
/// branch, matching the [`pmResult`] modeling).
pub fn Platform_gettime_monotonic(msec: &mut u64) {
    unsafe {
        let p = &*pcp.load(Ordering::Relaxed);
        if !p.result.is_null() {
            // #if PMAPI_VERSION >= 3
            *msec = ((*p.result).timestamp.tv_sec as u64 * 1000)
                + ((*p.result).timestamp.tv_nsec as u64 / 1000000);
        } else {
            *msec = 0;
        }
    }
}

/// Port of `Hashtable* Platform_dynamicMeters(void)` (`Platform.c:931`) — the
/// global dynamic-meter registry (`pcp->meters.table`, null before init).
pub fn Platform_dynamicMeters() -> *mut Hashtable {
    unsafe {
        let p = &mut *pcp.load(Ordering::Relaxed);
        match p.meters.table.as_mut() {
            Some(t) => t as *mut Hashtable,
            None => ptr::null_mut(),
        }
    }
}

/// Port of `void Platform_dynamicMeterInit(Meter* meter)` (`Platform.c:935`).
pub fn Platform_dynamicMeterInit(meter: &mut Meter) {
    unsafe {
        let p = &mut *pcp.load(Ordering::Relaxed);
        if let Some(t) = p.meters.table.as_ref() {
            if let Some(this) = Hashtable_get(t, meter.param)
                .and_then(|o| (o as &dyn Any).downcast_ref::<PCPDynamicMeter>())
            {
                PCPDynamicMeter_enable(this);
            }
        }
    }
}

/// Port of `void Platform_dynamicMeterUpdateValues(Meter* meter)`
/// (`Platform.c:941`).
pub fn Platform_dynamicMeterUpdateValues(meter: &mut Meter) {
    unsafe {
        let p = &mut *pcp.load(Ordering::Relaxed);
        if let Some(t) = p.meters.table.as_ref() {
            if let Some(this) = Hashtable_get(t, meter.param)
                .and_then(|o| (o as &dyn Any).downcast_ref::<PCPDynamicMeter>())
            {
                PCPDynamicMeter_updateValues(this, meter);
            }
        }
    }
}

/// Port of `void Platform_dynamicMeterDisplay(const Meter* meter, RichString*
/// out)` (`Platform.c:947`).
pub fn Platform_dynamicMeterDisplay(meter: &Meter, out: &mut RichString) {
    unsafe {
        let p = &mut *pcp.load(Ordering::Relaxed);
        if let Some(t) = p.meters.table.as_ref() {
            if let Some(this) = Hashtable_get(t, meter.param)
                .and_then(|o| (o as &dyn Any).downcast_ref::<PCPDynamicMeter>())
            {
                PCPDynamicMeter_display(this, meter, out);
            }
        }
    }
}

/// Port of `Hashtable* Platform_dynamicColumns(void)` (`Platform.c:953`) — the
/// global dynamic-column registry (`pcp->columns.table`, null before init).
pub fn Platform_dynamicColumns() -> *mut Hashtable {
    unsafe {
        let p = &mut *pcp.load(Ordering::Relaxed);
        match p.columns.table.as_mut() {
            Some(t) => t as *mut Hashtable,
            None => ptr::null_mut(),
        }
    }
}

/// Port of `const char* Platform_dynamicColumnName(unsigned int key)`
/// (`Platform.c:957`). Enables the column's metric and returns its
/// caption/heading/name. The C returns the internal `char*`; the port returns an
/// owned `String` clone (no live caller needs the aliased pointer).
pub fn Platform_dynamicColumnName(key: u32) -> Option<String> {
    unsafe {
        let p = &mut *pcp.load(Ordering::Relaxed);
        let t = p.columns.table.as_ref()?;
        let this = (Hashtable_get(t, key)? as &dyn Any).downcast_ref::<PCPDynamicColumn>()?;
        let metric = Metric_fromId(this.id);
        Metric_enable(metric, true);
        if let Some(caption) = this.super_.caption.as_deref() {
            return Some(caption.to_string());
        }
        if let Some(heading) = this.super_.heading.as_deref() {
            return Some(heading.to_string());
        }
        Some(this.super_.name.clone())
    }
}

/// Port of `bool Platform_dynamicColumnWriteField(const Process* proc,
/// RichString* str, unsigned int key)` (`Platform.c:971`). The C `const
/// Process*` (cast to `PCPProcess*` by `PCPDynamicColumn_writeField`) is taken
/// directly as `&PCPProcess` (the caller has the concrete type).
pub fn Platform_dynamicColumnWriteField(proc: &PCPProcess, str: &mut RichString, key: u32) -> bool {
    unsafe {
        let p = &mut *pcp.load(Ordering::Relaxed);
        if let Some(t) = p.columns.table.as_ref() {
            if let Some(this) = Hashtable_get(t, key)
                .and_then(|o| (o as &dyn Any).downcast_ref::<PCPDynamicColumn>())
            {
                PCPDynamicColumn_writeField(this, proc, str);
                return true;
            }
        }
        false
    }
}

/// Port of `Hashtable* Platform_dynamicScreens(void)` (`Platform.c:980`) — the
/// global dynamic-screen registry (`pcp->screens.table`, null before init).
pub fn Platform_dynamicScreens() -> *mut Hashtable {
    unsafe {
        let p = &mut *pcp.load(Ordering::Relaxed);
        match p.screens.table.as_mut() {
            Some(t) => t as *mut Hashtable,
            None => ptr::null_mut(),
        }
    }
}

/// Port of `void Platform_defaultDynamicScreens(Settings* settings)`
/// (`Platform.c:984`).
pub fn Platform_defaultDynamicScreens(settings: &mut Settings) {
    unsafe {
        let p = &*pcp.load(Ordering::Relaxed);
        PCPDynamicScreen_appendScreens(&p.screens, settings);
    }
}

/// Port of `void Platform_addDynamicScreen(ScreenSettings* ss)`
/// (`Platform.c:988`).
pub fn Platform_addDynamicScreen(ss: &mut ScreenSettings) {
    unsafe {
        let p = &*pcp.load(Ordering::Relaxed);
        PCPDynamicScreen_addDynamicScreen(&p.screens, ss);
    }
}

/// Port of `void Platform_addDynamicScreenAvailableColumns(Panel*
/// availableColumns, const char* screen)` (`Platform.c:992`).
pub fn Platform_addDynamicScreenAvailableColumns(availableColumns: &mut Panel, screen: &str) {
    unsafe {
        let p = &*pcp.load(Ordering::Relaxed);
        if let Some(screens) = p.screens.table.as_ref() {
            PCPDynamicScreens_addAvailableColumns(availableColumns, screens, screen);
        }
    }
}

/// Port of `void Platform_updateTables(Machine* host)` (`Platform.c:997`).
/// Rebuilds the per-screen instance-domain tables and the dynamic-column widths.
pub fn Platform_updateTables(host: &mut Machine) {
    unsafe {
        let p = &mut *pcp.load(Ordering::Relaxed);
        PCPDynamicScreen_appendTables(&p.screens, host as *const Machine);
        PCPDynamicColumns_setupWidths(&mut p.columns);
    }
}
