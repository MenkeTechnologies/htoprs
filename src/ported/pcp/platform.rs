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
