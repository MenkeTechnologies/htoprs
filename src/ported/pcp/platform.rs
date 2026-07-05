//! Port of `pcp/Platform.c` + `.h` — the PCP platform backend's global state.
//!
//! Only the metric-registry half of the `Platform` struct (`pcp/Platform.h:45`)
//! and the `pcp` global (`Platform.c:57`) are modeled here so far — the fields
//! [`Metric`](super::metric) reads/writes. `Platform` is htop's own struct (not
//! a libpcp type), so it is modeled idiomatically (owned `Vec`s for the C
//! `xCalloc`'d `pmID*`/`pmDesc*` arrays) rather than by C layout. The dynamic
//! meter/column/screen tables and the archive/uname tail fields, plus the
//! `Platform_*` functions, are added when the rest of `Platform.c` is ported.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use std::os::raw::{c_char, c_int};
use std::ptr;
use std::sync::atomic::AtomicPtr;

use crate::ported::hashtable::Hashtable;
use crate::ported::machine::Machine;
use crate::ported::pcp::metric::Metric;
use crate::ported::pcp::pmapi::{pmDesc, pmID, pmResult};

/// Port of `typedef struct Platform_` (`pcp/Platform.h:45`) — the PCP backend's
/// global state. Partial: the metric-registry fields consumed by
/// [`Metric`](super::metric). The C `xCalloc`'d `names`/`pmids`/`fetch`/`descs`
/// arrays (indexed by `Metric`) are owned `Vec`s; `result` is the libpcp-owned
/// `pmFetch` output (a raw pointer, freed via `pmFreeResult`).
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
    // Deferred (added when the rest of `Platform.c` is ported): the
    // `PCPDynamicMeters/Columns/Screens` tables, and the archive `offset` /
    // `btime` / `release` / `pidmax` / `ncpu` tail fields.
}

/// Port of `Platform* pcp` (`pcp/Platform.c:57`) — the single global PCP backend
/// state, set up by `Platform_init` (a leaked `Box`, as the C `xCalloc`'d global
/// lives for the program's lifetime). Null until initialized; [`Metric`] loads
/// and dereferences it. Modeled as an `AtomicPtr` (the `CRT_*` global pattern).
pub static pcp: AtomicPtr<Platform> = AtomicPtr::new(ptr::null_mut());

/// TODO: port of `unsigned int Platform_getMaxCPU(void)` (`pcp/Platform.c:508`).
/// Caches the processor count into the (deferred) `pcp->ncpu` field; needs the
/// PCP context setup the rest of `Platform.c` provides. Scaffolded here so
/// [`PCPMachine`](super::pcpmachine)'s call site stays 1:1 until `Platform.c` is
/// ported.
pub fn Platform_getMaxCPU() -> u32 {
    todo!("pcp/Platform.c:508 Platform_getMaxCPU — not yet ported (needs pcp->ncpu)")
}

/// TODO: port of `void Platform_updateTables(Machine* host)`
/// (`pcp/Platform.c:997`). Rebuilds the PCP dynamic meter/column/screen tables
/// (the deferred `Platform` fields). Scaffolded here so
/// [`PCPMachine`](super::pcpmachine)'s `Machine_new` call site stays 1:1 until
/// `Platform.c` is ported.
pub fn Platform_updateTables(host: &mut Machine) {
    let _ = host;
    todo!("pcp/Platform.c:997 Platform_updateTables — not yet ported (dynamic tables)")
}

/// TODO: port of `time_t Platform_getBootTime(void)` (`pcp/Platform.c`). Returns
/// the boot time (seconds since epoch) read from the PCP context; needs the
/// context setup the rest of `Platform.c` provides. Scaffolded here so
/// [`PCPProcessTable`](super::pcpprocesstable)'s starttime call site stays 1:1
/// until `Platform.c` is ported.
pub fn Platform_getBootTime() -> libc::time_t {
    todo!("pcp/Platform.c Platform_getBootTime — not yet ported (needs PCP context)")
}

/// TODO: port of `size_t Platform_addMetric(Metric id, const char* name)`
/// (`pcp/Platform.c:328`). Registers a metric name into the `pcp->names`/`pmids`
/// registry and returns its result offset; needs the `Platform_init` context
/// setup the rest of `Platform.c` provides. Scaffolded here so
/// [`PCPDynamicColumn`](super::pcpdynamiccolumn)'s `addMetric` call site stays
/// 1:1 until `Platform.c` is ported.
pub fn Platform_addMetric(id: Metric, name: &str) -> usize {
    let _ = (id, name);
    todo!("pcp/Platform.c:328 Platform_addMetric — not yet ported (needs the metric registry)")
}

/// TODO: port of `Hashtable* Platform_dynamicColumns(void)` (`pcp/Platform.c`).
/// Returns the global dynamic-column registry (`pcp->columns.table`, a deferred
/// `Platform` field). Scaffolded here so [`Instance`](super::instance)'s
/// `compareByKey`/`writeField` call sites stay 1:1 until `Platform.c` is ported.
pub fn Platform_dynamicColumns() -> *mut Hashtable {
    todo!("pcp/Platform.c Platform_dynamicColumns — not yet ported (needs pcp->columns)")
}
