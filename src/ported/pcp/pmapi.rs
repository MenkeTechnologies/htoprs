//! Hand-declared libpcp / PMAPI FFI surface — the foundation the PCP backend's
//! `Metric` / `Platform` layers build on. Not an htop C file; it is the Rust
//! analog of `#include <pcp/pmapi.h>`, exactly as the DragonFly backend
//! hand-declares the `kvm_*` surface `libc` does not expose. Every type,
//! constant, and function is transcribed verbatim from the Performance
//! Co-Pilot source (`performancecopilot/pcp` `src/include/pcp/pmapi.h`).
//!
//! Confined to the `pcp` cargo feature. The `extern` block links `libpcp`
//! (`#[link(name = "pcp")]`); with the feature enabled on a host without PCP
//! installed the link step fails by design (htop's `--enable-pcp` likewise
//! requires libpcp) — the FFI-consuming modules are verified by primary-source
//! reading + the port-purity gate, the tier-3 model.
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(dead_code)]

use std::os::raw::{c_char, c_int, c_uint};

/// `typedef unsigned int pmID` (`pmapi.h:86`) — metric identifier.
pub type pmID = c_uint;
/// `typedef unsigned int pmInDom` (`pmapi.h:89`) — instance-domain identifier.
pub type pmInDom = c_uint;

/// `#define PM_INDOM_NULL 0xffffffff` (`pmapi.h:90`).
pub const PM_INDOM_NULL: pmInDom = 0xffff_ffff;
/// `#define PM_IN_NULL 0xffffffff` (`pmapi.h:91`) — used as an `int` instance id.
pub const PM_IN_NULL: c_int = 0xffff_ffffu32 as c_int;

// Base data types (`pmapi.h:187-194`), the `pmDesc.type` / `pmExtractValue`
// out-type codes.
pub const PM_TYPE_NOSUPPORT: c_int = -1;
pub const PM_TYPE_32: c_int = 0;
pub const PM_TYPE_U32: c_int = 1;
pub const PM_TYPE_64: c_int = 2;
pub const PM_TYPE_U64: c_int = 3;
pub const PM_TYPE_FLOAT: c_int = 4;
pub const PM_TYPE_DOUBLE: c_int = 5;
pub const PM_TYPE_STRING: c_int = 6;

/// `#define PM_VAL_INSITU 0` (`pmapi.h:547`) — `pmValue.value.lval` is the value.
pub const PM_VAL_INSITU: c_int = 0;

/// `typedef union pmAtomValue` (`pmapi.h`) — a single metric value in one of the
/// PM_TYPE_* representations.
#[repr(C)]
#[derive(Clone, Copy)]
pub union pmAtomValue {
    /// 32-bit signed (`PM_TYPE_32`).
    pub l: i32,
    /// 32-bit unsigned (`PM_TYPE_U32`).
    pub ul: u32,
    /// 64-bit signed (`PM_TYPE_64`).
    pub ll: i64,
    /// 64-bit unsigned (`PM_TYPE_U64`).
    pub ull: u64,
    /// 32-bit float (`PM_TYPE_FLOAT`).
    pub f: f32,
    /// 64-bit double (`PM_TYPE_DOUBLE`).
    pub d: f64,
    /// char pointer (`PM_TYPE_STRING`; caller frees).
    pub cp: *mut c_char,
    /// value-block pointer (aggregate/event types).
    pub vbp: *mut pmValueBlock,
}

/// `typedef struct pmValueBlock` (`pmapi.h`). The C leading `vtype:8`/`vlen:24`
/// bitfield is one 32-bit word (`word`); `vbuf` is the flexible value payload.
#[repr(C)]
pub struct pmValueBlock {
    /// Packed `vtype` (high 8 bits) + `vlen` (low 24 bits); bit order is
    /// host-endian per the C `HAVE_BITFIELDS_LTOR` split — opaque here.
    pub word: u32,
    /// `char vbuf[1]` — flexible value bytes.
    pub vbuf: [c_char; 1],
}

/// The `value` member of [`pmValue`] — either an in-situ int (`PM_VAL_INSITU`)
/// or a pointer to a [`pmValueBlock`].
#[repr(C)]
#[derive(Clone, Copy)]
pub union pmValue_value {
    /// Pointer to the value block (`valfmt != PM_VAL_INSITU`).
    pub pval: *mut pmValueBlock,
    /// In-situ 32-bit value (`valfmt == PM_VAL_INSITU`).
    pub lval: c_int,
}

/// `typedef struct pmValue` (`pmapi.h`) — one instance's value within a
/// [`pmValueSet`].
#[repr(C)]
pub struct pmValue {
    /// Instance identifier.
    pub inst: c_int,
    /// The value (in-situ int or block pointer, discriminated by `valfmt`).
    pub value: pmValue_value,
}

/// `typedef struct pmValueSet` (`pmapi.h`) — all instances of one metric.
/// `vlist` is a C flexible array (`pmValue vlist[1]`): index `> 0` via pointer
/// arithmetic over `numval` entries.
#[repr(C)]
pub struct pmValueSet {
    /// Metric identifier.
    pub pmid: pmID,
    /// Number of values (instances), or a negative error code.
    pub numval: c_int,
    /// Value encoding (`PM_VAL_INSITU` / `PM_VAL_DPTR` / `PM_VAL_SPTR`).
    pub valfmt: c_int,
    /// Flexible array of `numval` `pmValue`s.
    pub vlist: [pmValue; 1],
}

/// `typedef struct pmResult` (`pmapi.h`) — the result of `pmFetch`, one
/// [`pmValueSet`] per requested PMID. `vset` is a C flexible array
/// (`pmValueSet* vset[1]`) of `numpmid` pointers. `timestamp` is 16 bytes on
/// LP64 (`timespec`/`timeval` alike), so `numpmid`/`vset` offsets are
/// version-stable.
#[repr(C)]
pub struct pmResult {
    /// Collector time stamp.
    pub timestamp: libc::timespec,
    /// Number of PMIDs.
    pub numpmid: c_int,
    /// Flexible array of `numpmid` value-set pointers.
    pub vset: [*mut pmValueSet; 1],
}

/// `typedef struct pmUnits` (`pmapi.h`) — an 8×4-bit + 5+3 bitfield packed into
/// one 32-bit word. htop reads no sub-field of it (only `pmDesc.type`), so it is
/// modeled opaquely to preserve `pmDesc`'s size/layout.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct pmUnits {
    /// The packed dimension/scale bitfields.
    pub bits: u32,
}

/// `typedef struct pmDesc` (`pmapi.h`) — a metric's descriptor.
#[repr(C)]
pub struct pmDesc {
    /// Unique metric identifier.
    pub pmid: pmID,
    /// Base data type (`PM_TYPE_*`).
    pub type_: c_int,
    /// Instance domain.
    pub indom: pmInDom,
    /// Value semantics.
    pub sem: c_int,
    /// Dimension and units.
    pub units: pmUnits,
}

#[link(name = "pcp")]
extern "C" {
    /// `int pmFetch(int, pmID*, pmResult**)` (`pmapi.h:593`).
    pub fn pmFetch(numpmid: c_int, pmidlist: *mut pmID, result: *mut *mut pmResult) -> c_int;
    /// `int pmLookupName(int, const char**, pmID*)` (`pmapi.h:323`).
    pub fn pmLookupName(
        numpmid: c_int,
        namelist: *const *const c_char,
        pmidlist: *mut pmID,
    ) -> c_int;
    /// `int pmLookupText(pmID, int, char**)` (`pmapi.h:766`).
    pub fn pmLookupText(pmid: pmID, level: c_int, buffer: *mut *mut c_char) -> c_int;
    /// `int pmNameInDom(pmInDom, int, char**)` (`pmapi.h:366`).
    pub fn pmNameInDom(indom: pmInDom, inst: c_int, name: *mut *mut c_char) -> c_int;
    /// `int pmExtractValue(int, const pmValue*, int, pmAtomValue*, int)`
    /// (`pmapi.h:743`).
    pub fn pmExtractValue(
        valfmt: c_int,
        ival: *const pmValue,
        itype: c_int,
        oval: *mut pmAtomValue,
        otype: c_int,
    ) -> c_int;
    /// `void pmFreeResult(pmResult*)` (`pmapi.h:740`).
    pub fn pmFreeResult(result: *mut pmResult);
    /// `char* pmErrStr(int)` (`pmapi.h:294`; not thread-safe).
    pub fn pmErrStr(code: c_int) -> *mut c_char;
    /// `int pmReconnectContext(int)` (`pmapi.h:433`).
    pub fn pmReconnectContext(handle: c_int) -> c_int;
    /// `int pmStore(const pmResult*)` (`pmapi.h:763`).
    pub fn pmStore(result: *const pmResult) -> c_int;
    /// `int pmUseContext(int)` (`pmapi.h:427`).
    pub fn pmUseContext(handle: c_int) -> c_int;
    /// `int pmNewContext(int, const char*)` (`pmapi.h:400`).
    pub fn pmNewContext(type_: c_int, name: *const c_char) -> c_int;
}
