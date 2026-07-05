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

/// `#define PM_ID_NULL 0xffffffff` (`pmapi.h:87`) — a disabled/absent PMID.
pub const PM_ID_NULL: pmID = 0xffff_ffff;

// Context types (`pmapi.h:401-404`), the `pmNewContext`/`pmOptions.context`
// values.
pub const PM_CONTEXT_UNDEF: c_int = -1;
pub const PM_CONTEXT_HOST: c_int = 1;
pub const PM_CONTEXT_ARCHIVE: c_int = 2;
pub const PM_CONTEXT_LOCAL: c_int = 3;

// Space scale codes (`pmapi.h:128-129`), the `pmUnits.scaleSpace` values.
pub const PM_SPACE_BYTE: c_int = 0;
pub const PM_SPACE_KBYTE: c_int = 1;

// Time scale codes (`pmapi.h:138-143`), the `pmUnits.scaleTime` values.
pub const PM_TIME_NSEC: c_int = 0;
pub const PM_TIME_USEC: c_int = 1;
pub const PM_TIME_MSEC: c_int = 2;
pub const PM_TIME_SEC: c_int = 3;
pub const PM_TIME_MIN: c_int = 4;
pub const PM_TIME_HOUR: c_int = 5;

/// `#define PM_TEXT_ONELINE 1` (`pmapi.h:768`).
pub const PM_TEXT_ONELINE: c_int = 1;

/// `#define PM_ERR_IPC (-PM_ERR_BASE-21)` (`pmapi.h:233`) with
/// `PM_ERR_BASE = PM_ERR_BASE2 = 12345` (`pmapi.h:207-208`) ⇒ `-12366`.
pub const PM_ERR_IPC: c_int = -12366;

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

impl pmUnits {
    /// `pmUnits.scaleSpace` (`unsigned int : 4`). On the little-endian targets
    /// (`HAVE_BITFIELDS_LTOR` undefined, `pmapi.h:106` `#else` layout), the
    /// fields pack LSB-first as `extraScale:3, extraUnit:5, scaleCount:4,
    /// scaleTime:4, scaleSpace:4, …`, so `scaleSpace` occupies bits 16–19.
    #[inline]
    pub fn scaleSpace(self) -> c_int {
        ((self.bits >> 16) & 0xF) as c_int
    }

    /// `pmUnits.scaleTime` (`unsigned int : 4`) — bits 12–15 (see
    /// [`pmUnits::scaleSpace`] for the bit-order derivation).
    #[inline]
    pub fn scaleTime(self) -> c_int {
        ((self.bits >> 12) & 0xF) as c_int
    }
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

/// `typedef struct pmLongOptions` (`pmapi.h:1025`) — one CLI long-option
/// descriptor in a `pmOptions.long_options` table (getopt-`long`-shaped, plus
/// `argname`/`message` for `pmUsageMessage`).
#[repr(C)]
pub struct pmLongOptions {
    /// `const char* long_opt` — the `--name` (NULL terminates the table).
    pub long_opt: *const c_char,
    /// `int has_arg` — `no_argument`/`required_argument`/`optional_argument`.
    pub has_arg: c_int,
    /// `int short_opt` — the equivalent short-option character / return value.
    pub short_opt: c_int,
    /// `const char* argname` — argument placeholder for the usage message.
    pub argname: *const c_char,
    /// `const char* message` — the help text for the usage message.
    pub message: *const c_char,
}

/// `typedef int (*pmOptionOverride)(int, struct pmOptions*)` (`pmapi.h:1023`) —
/// caller hook to override option handling. Nullable (`Option<fn>` ⇒ the C
/// NULL function pointer).
pub type pmOptionOverride = Option<unsafe extern "C" fn(c_int, *mut pmOptions) -> c_int>;

/// `typedef struct pmOptions` (`pmapi.h:1033`, the current/v3 ABI) — the shared
/// PMAPI command-line option-parsing state (`pmGetOptions`/
/// `pmGetContextOptions` fill it, htop reads the parsed `context`/`hosts`/
/// `archives`/`timezone`/`errors`). Transcribed field-for-field; the trailing
/// `guiflag:1 | tzflag:1 | nsflag:1 | Lflag:1 | zeroes:28` bitfields are one
/// `unsigned int` word ([`bitfields`](Self::bitfields)) with LSB-first packing
/// (`HAVE_BITFIELDS_LTOR` undefined, matching [`pmUnits`]), read/written via the
/// [`tzflag`](Self::tzflag)/[`guiflag`](Self::guiflag) accessors.
#[repr(C)]
pub struct pmOptions {
    /// `int version`.
    pub version: c_int,
    /// `int flags` — `PM_OPTFLAG_*`.
    pub flags: c_int,
    /// `const char* short_options` — in: the getopt short-option string.
    pub short_options: *const c_char,
    /// `pmLongOptions* long_options` — in: the long-option table.
    pub long_options: *mut pmLongOptions,
    /// `const char* short_usage` — in: the one-line usage synopsis.
    pub short_usage: *const c_char,
    /// `pmOptionOverride override` — in: the general option-override hook.
    pub override_: pmOptionOverride,
    /// `int index` — out: getopt long-option index.
    pub index: c_int,
    /// `int optind` — out: getopt `optind`.
    pub optind: c_int,
    /// `int opterr` — out: getopt `opterr`.
    pub opterr: c_int,
    /// `int optopt` — out: getopt `optopt`.
    pub optopt: c_int,
    /// `char* optarg` — out: getopt `optarg`.
    pub optarg: *mut c_char,
    /// `int __initialized` — internal; do not access.
    pub __initialized: c_int,
    /// `char* __nextchar` — internal; do not access.
    pub __nextchar: *mut c_char,
    /// `int __ordering` — internal; do not access.
    pub __ordering: c_int,
    /// `int __posixly_correct` — internal; do not access.
    pub __posixly_correct: c_int,
    /// `int __first_nonopt` — internal; do not access.
    pub __first_nonopt: c_int,
    /// `int __last_nonopt` — internal; do not access.
    pub __last_nonopt: c_int,
    /// `int errors` — out: parse-error count.
    pub errors: c_int,
    /// `int context` — out: `PM_CONTEXT_{HOST,ARCHIVE,LOCAL}`.
    pub context: c_int,
    /// `int nhosts` — out: number of `hosts` entries.
    pub nhosts: c_int,
    /// `int narchives` — out: number of `archives` entries.
    pub narchives: c_int,
    /// `char** hosts` — out: the requested host specifications.
    pub hosts: *mut *mut c_char,
    /// `char** archives` — out: the requested archive paths.
    pub archives: *mut *mut c_char,
    /// `struct timespec start` — out: sample window start.
    pub start: libc::timespec,
    /// `struct timespec finish` — out: sample window finish.
    pub finish: libc::timespec,
    /// `struct timespec origin` — out: sample window origin.
    pub origin: libc::timespec,
    /// `struct timespec interval` — out: sample interval.
    pub interval: libc::timespec,
    /// `char* align_optarg`.
    pub align_optarg: *mut c_char,
    /// `char* start_optarg`.
    pub start_optarg: *mut c_char,
    /// `char* finish_optarg`.
    pub finish_optarg: *mut c_char,
    /// `char* origin_optarg`.
    pub origin_optarg: *mut c_char,
    /// `char* guiport_optarg`.
    pub guiport_optarg: *mut c_char,
    /// `char* timezone` — out: the requested reporting timezone (`-Z`).
    pub timezone: *mut c_char,
    /// `int samples`.
    pub samples: c_int,
    /// `int guiport`.
    pub guiport: c_int,
    /// `int padding`.
    pub padding: c_int,
    /// The `guiflag:1 | tzflag:1 | nsflag:1 | Lflag:1 | zeroes:28` bitfields
    /// packed into one `unsigned int` word (LSB-first). Accessed via
    /// [`tzflag`](Self::tzflag)/[`guiflag`](Self::guiflag)/
    /// [`set_tzflag`](Self::set_tzflag).
    pub bitfields: c_uint,
}

impl pmOptions {
    /// `guiflag : 1` — bit 0 (LSB-first, see [`pmUnits::scaleSpace`]).
    #[inline]
    pub fn guiflag(&self) -> c_uint {
        self.bitfields & 1
    }

    /// `tzflag : 1` — bit 1. Set when `--hostzone`/`-z` was requested.
    #[inline]
    pub fn tzflag(&self) -> c_uint {
        (self.bitfields >> 1) & 1
    }

    /// Write `tzflag : 1` (bit 1) — the C `opts.tzflag = v`.
    #[inline]
    pub fn set_tzflag(&mut self, v: c_uint) {
        self.bitfields = (self.bitfields & !(1u32 << 1)) | ((v & 1) << 1);
    }
}

/// A fully zero-initialized [`pmOptions`] — the Rust analog of the C
/// `pmOptions opts;` file-scope global (BSS zero-init). Used to seed the
/// backend's `opts` global before `pmGetOptions` fills it.
pub const PMOPTIONS_ZERO: pmOptions = pmOptions {
    version: 0,
    flags: 0,
    short_options: core::ptr::null(),
    long_options: core::ptr::null_mut(),
    short_usage: core::ptr::null(),
    override_: None,
    index: 0,
    optind: 0,
    opterr: 0,
    optopt: 0,
    optarg: core::ptr::null_mut(),
    __initialized: 0,
    __nextchar: core::ptr::null_mut(),
    __ordering: 0,
    __posixly_correct: 0,
    __first_nonopt: 0,
    __last_nonopt: 0,
    errors: 0,
    context: 0,
    nhosts: 0,
    narchives: 0,
    hosts: core::ptr::null_mut(),
    archives: core::ptr::null_mut(),
    start: libc::timespec { tv_sec: 0, tv_nsec: 0 },
    finish: libc::timespec { tv_sec: 0, tv_nsec: 0 },
    origin: libc::timespec { tv_sec: 0, tv_nsec: 0 },
    interval: libc::timespec { tv_sec: 0, tv_nsec: 0 },
    align_optarg: core::ptr::null_mut(),
    start_optarg: core::ptr::null_mut(),
    finish_optarg: core::ptr::null_mut(),
    origin_optarg: core::ptr::null_mut(),
    guiport_optarg: core::ptr::null_mut(),
    timezone: core::ptr::null_mut(),
    samples: 0,
    guiport: 0,
    padding: 0,
    bitfields: 0,
};

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
    /// `double pmtimevalToReal(const struct timeval*)` (`pmapi.h`) — a `timeval`
    /// as fractional seconds.
    pub fn pmtimevalToReal(tv: *const libc::timeval) -> f64;
    /// `int pmConvScale(int, const pmAtomValue*, const pmUnits*, pmAtomValue*,
    /// const pmUnits*)` (`pmapi.h:749`) — rescale a value between units.
    pub fn pmConvScale(
        type_: c_int,
        ival: *const pmAtomValue,
        iunit: *const pmUnits,
        oval: *mut pmAtomValue,
        ounit: *const pmUnits,
    ) -> c_int;
    /// `char* pmGetConfig(const char*)` (`pmapi.h:892`) — a PCP config value
    /// (e.g. `PCP_SHARE_DIR`).
    pub fn pmGetConfig(name: *const c_char) -> *mut c_char;
    /// `char* pmGetProgname(void)` (`pmapi.h:1301`) — the program name.
    pub fn pmGetProgname() -> *mut c_char;
    /// `int pmRegisterDerivedMetric(const char*, const char*, char**)`
    /// (`pmapi.h:1099`) — register a derived-metric expression.
    pub fn pmRegisterDerivedMetric(
        name: *const c_char,
        expr: *const c_char,
        errmsg: *mut *mut c_char,
    ) -> c_int;
    /// `int pmsprintf(char*, size_t, const char*, ...)` (`pmapi.h:855`) — PCP's
    /// bounded, always-NUL-terminating `snprintf`.
    pub fn pmsprintf(buf: *mut c_char, size: usize, fmt: *const c_char, ...) -> c_int;
    /// `int pmLookupDesc(pmID, pmDesc*)` (`pmapi.h:350`) — fetch one metric's
    /// descriptor. Used by the htop `pmLookupDescs` fallback wrapper (the
    /// `#ifndef HAVE_PMLOOKUPDESCS` branch) ported in `platform.rs`.
    pub fn pmLookupDesc(pmid: pmID, desc: *mut pmDesc) -> c_int;
    /// `const char* pmGetContextHostName(int)` (`pmapi.h:383`) — the hostname of
    /// the metric source for a context (libpcp-owned static/thread buffer).
    pub fn pmGetContextHostName(handle: c_int) -> *const c_char;
    /// `int pmDestroyContext(int)` (`pmapi.h:394`) — tear down a PMAPI context.
    pub fn pmDestroyContext(handle: c_int) -> c_int;
    /// `const char* pmIDStr(pmID)` (`pmapi.h:786`; not thread-safe) — render a
    /// PMID as text. Referenced only by the omitted `pmDebugOptions.appl0` debug
    /// branch of the `pmLookupDescs` wrapper.
    pub fn pmIDStr(pmid: pmID) -> *const c_char;
    /// `int pmflush(void)` (`pmapi.h:849`) — flush the `pmprintf` message buffer.
    pub fn pmflush() -> c_int;
    /// `int pmprintf(const char*, ...)` (`pmapi.h:848`) — append to libpcp's
    /// deferred message buffer (variadic).
    pub fn pmprintf(fmt: *const c_char, ...) -> c_int;
    /// `void pmtimevalDec(struct timeval*, const struct timeval*)`
    /// (`pmapi.h:1358`) — subtract `*bp` from `*ap` in place.
    pub fn pmtimevalDec(ap: *mut libc::timeval, bp: *const libc::timeval);
    /// `int pmGetOptions(int, char* const*, pmOptions*)` (`pmapi.h:1090`) — parse
    /// the standard PCP command-line options into `opts`. (The header
    /// `#define pmGetOptions pmGetOptions_v2` is the v2-ABI compat path; the
    /// plain symbol targets the current ABI, matching the [`pmOptions`] layout.)
    pub fn pmGetOptions(argc: c_int, argv: *const *mut c_char, opts: *mut pmOptions) -> c_int;
    /// `int pmGetContextOptions(int, pmOptions*)` (`pmapi.h:1091`) — apply the
    /// per-context options (timezones, time window) after `pmNewContext`.
    pub fn pmGetContextOptions(ctx: c_int, opts: *mut pmOptions) -> c_int;
    /// `void pmUsageMessage(pmOptions*)` (`pmapi.h:1092`) — print the standard
    /// usage message built from `opts`.
    pub fn pmUsageMessage(opts: *mut pmOptions);
}
