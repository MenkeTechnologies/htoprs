//! Port of `pcp/Metric.c` + the `Metric` enum from `pcp/Metric.h` — htop's
//! thin wrapper layer over the libpcp/PMAPI value-fetching surface.
//!
//! 1:1 faithful port; the C is the spec. The libpcp types/constants/externs are
//! reused from [`crate::ported::pcp::pmapi`] (the hand-declared PMAPI FFI, the
//! DragonFly-kvm precedent) — nothing is redeclared here. The C `pcp` global
//! (`extern Platform* pcp`) is [`crate::ported::pcp::platform::pcp`], loaded and
//! dereferenced exactly as the C assumes non-null (a null deref is the faithful
//! "not initialized" crash).
//!
//! The C reads several structs as flexible arrays (`pmResult.vset[1]`,
//! `pmValueSet.vlist[1]`) that really hold N entries: those are indexed via
//! pointer arithmetic (`.as_ptr().add(i)`), never Rust `[]` (which would
//! bounds-check against the declared length of 1). Union fields
//! (`pmAtomValue.ull`, `pmValue.value.lval`) are read/written inside `unsafe`.
//!
//! Confined to the `pcp` cargo feature (this whole sub-tree is
//! `#[cfg(feature = "pcp")]`). It won't link libpcp on macOS; verified by
//! `cargo check --features pcp` + primary-source reading + the port-purity gate.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(dead_code)]

use core::ffi::{c_long, CStr};
use std::os::raw::{c_char, c_int};
use std::ptr;
use std::sync::atomic::Ordering;

use crate::ported::pcp::platform;
use crate::ported::pcp::pmapi::{
    pmAtomValue, pmDesc, pmExtractValue, pmFetch, pmFreeResult, pmID, pmLookupName, pmLookupText,
    pmNameInDom, pmReconnectContext, pmResult, pmStore, pmValue, pmValueSet, pmValue_value,
    PM_ERR_IPC, PM_ID_NULL, PM_IN_NULL, PM_SPACE_KBYTE, PM_TEXT_ONELINE, PM_TIME_HOUR, PM_TIME_MIN,
    PM_TIME_MSEC, PM_TIME_NSEC, PM_TIME_SEC, PM_TIME_USEC, PM_TYPE_U64, PM_VAL_INSITU,
};

/// Port of `#define ONE_K 1024` (`Macros.h`) — the KiB power-of-two factor the
/// `kibibytes` scaler multiplies/divides by.
const ONE_K: u64 = 1024;

/// Port of `typedef enum Metric_` (`pcp/Metric.h:25`) — the PCP backend's metric
/// registry, used as an index into the `pcp->pmids/fetch/descs/names` arrays. A
/// C enum auto-increments from 0, so `PCP_CONTROL_THREADS == 0` and
/// `PCP_METRIC_COUNT` is the total count. `#[repr(usize)]` because it is used as
/// an array index.
#[repr(usize)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Metric {
    PCP_CONTROL_THREADS, // proc.control.perclient.threads

    PCP_HINV_NCPU,                 // hinv.ncpu
    PCP_HINV_NDISK,                // hinv.ndisk
    PCP_HINV_CPUCLOCK,             // hinv.cpu.clock
    PCP_UNAME_SYSNAME,             // kernel.uname.sysname
    PCP_UNAME_RELEASE,             // kernel.uname.release
    PCP_UNAME_MACHINE,             // kernel.uname.machine
    PCP_UNAME_DISTRO,              // kernel.uname.distro
    PCP_LOAD_AVERAGE,              // kernel.all.load
    PCP_PID_MAX,                   // kernel.all.pid_max
    PCP_UPTIME,                    // kernel.all.uptime
    PCP_BOOTTIME,                  // kernel.all.boottime
    PCP_CPU_USER,                  // kernel.all.cpu.user
    PCP_CPU_NICE,                  // kernel.all.cpu.nice
    PCP_CPU_SYSTEM,                // kernel.all.cpu.sys
    PCP_CPU_IDLE,                  // kernel.all.cpu.idle
    PCP_CPU_IOWAIT,                // kernel.all.cpu.wait.total
    PCP_CPU_IRQ,                   // kernel.all.cpu.intr
    PCP_CPU_SOFTIRQ,               // kernel.all.cpu.irq.soft
    PCP_CPU_STEAL,                 // kernel.all.cpu.steal
    PCP_CPU_GUEST,                 // kernel.all.cpu.guest
    PCP_CPU_GUESTNICE,             // kernel.all.cpu.guest_nice
    PCP_PERCPU_USER,               // kernel.percpu.cpu.user
    PCP_PERCPU_NICE,               // kernel.percpu.cpu.nice
    PCP_PERCPU_SYSTEM,             // kernel.percpu.cpu.sys
    PCP_PERCPU_IDLE,               // kernel.percpu.cpu.idle
    PCP_PERCPU_IOWAIT,             // kernel.percpu.cpu.wait.total
    PCP_PERCPU_IRQ,                // kernel.percpu.cpu.intr
    PCP_PERCPU_SOFTIRQ,            // kernel.percpu.cpu.irq.soft
    PCP_PERCPU_STEAL,              // kernel.percpu.cpu.steal
    PCP_PERCPU_GUEST,              // kernel.percpu.cpu.guest
    PCP_PERCPU_GUESTNICE,          // kernel.percpu.cpu.guest_nice
    PCP_MEM_TOTAL,                 // mem.physmem
    PCP_MEM_FREE,                  // mem.util.free
    PCP_MEM_ACTIVE,                // mem.util.active
    PCP_MEM_AVAILABLE,             // mem.util.available
    PCP_MEM_BUFFERS,               // mem.util.bufmem
    PCP_MEM_CACHED,                // mem.util.cached
    PCP_MEM_COMPRESSED,            // mem.util.compressed
    PCP_MEM_EXTERNAL,              // mem.util.external
    PCP_MEM_INACTIVE,              // mem.util.inactive
    PCP_MEM_SHARED,                // mem.util.shared
    PCP_MEM_PURGEABLE,             // mem.util.purgeable
    PCP_MEM_SPECULATIVE,           // mem.util.speculative
    PCP_MEM_SRECLAIM,              // mem.util.slabReclaimable
    PCP_MEM_WIRED,                 // mem.util.wired
    PCP_MEM_SWAPCACHED,            // mem.util.swapCached
    PCP_MEM_SWAPTOTAL,             // mem.util.swapTotal
    PCP_MEM_SWAPFREE,              // mem.util.swapFree
    PCP_SWAP_LENGTH,               // swap.length
    PCP_SWAP_FREE,                 // swap.free
    PCP_DISK_READB,                // disk.all.read_bytes
    PCP_DISK_WRITEB,               // disk.all.write_bytes
    PCP_DISK_ACTIVE,               // disk.all.avactive
    PCP_NET_RECVB,                 // network.all.in.bytes
    PCP_NET_SENDB,                 // network.all.out.bytes
    PCP_NET_RECVP,                 // network.all.in.packets
    PCP_NET_SENDP,                 // network.all.out.packets
    PCP_PSI_CPUSOME,               // kernel.all.pressure.cpu.some.avg
    PCP_PSI_IOSOME,                // kernel.all.pressure.io.some.avg
    PCP_PSI_IOFULL,                // kernel.all.pressure.io.full.avg
    PCP_PSI_IRQFULL,               // kernel.all.pressure.irq.full.avg
    PCP_PSI_MEMSOME,               // kernel.all.pressure.memory.some.avg
    PCP_PSI_MEMFULL,               // kernel.all.pressure.memory.full.avg
    PCP_ZFS_ARC_ANON_SIZE,         // zfs.arc.anon_size
    PCP_ZFS_ARC_BONUS_SIZE,        // zfs.arc.bonus_size
    PCP_ZFS_ARC_COMPRESSED_SIZE,   // zfs.arc.compressed_size
    PCP_ZFS_ARC_UNCOMPRESSED_SIZE, // zfs.arc.uncompressed_size
    PCP_ZFS_ARC_C_MIN,             // zfs.arc.c_min
    PCP_ZFS_ARC_C_MAX,             // zfs.arc.c_max
    PCP_ZFS_ARC_DBUF_SIZE,         // zfs.arc.dbuf_size
    PCP_ZFS_ARC_DNODE_SIZE,        // zfs.arc.dnode_size
    PCP_ZFS_ARC_HDR_SIZE,          // zfs.arc.hdr_size
    PCP_ZFS_ARC_MFU_SIZE,          // zfs.arc.mfu_size
    PCP_ZFS_ARC_MRU_SIZE,          // zfs.arc.mru_size
    PCP_ZFS_ARC_SIZE,              // zfs.arc.size
    PCP_ZRAM_CAPACITY,             // zram.capacity
    PCP_ZRAM_ORIGINAL,             // zram.mm_stat.data_size.original
    PCP_ZRAM_COMPRESSED,           // zram.mm_stat.data_size.compressed
    PCP_MEM_ZSWAP,                 // mem.util.zswap
    PCP_MEM_ZSWAPPED,              // mem.util.zswapped
    PCP_VFS_FILES_COUNT,           // vfs.files.count
    PCP_VFS_FILES_MAX,             // vfs.files.max

    PCP_PROC_PID,       // proc.psinfo.pid
    PCP_PROC_PPID,      // proc.psinfo.ppid
    PCP_PROC_TGID,      // proc.psinfo.tgid
    PCP_PROC_PGRP,      // proc.psinfo.pgrp
    PCP_PROC_SESSION,   // proc.psinfo.session
    PCP_PROC_STATE,     // proc.psinfo.sname
    PCP_PROC_TTY,       // proc.psinfo.tty
    PCP_PROC_TTYPGRP,   // proc.psinfo.tty_pgrp
    PCP_PROC_MINFLT,    // proc.psinfo.minflt
    PCP_PROC_MAJFLT,    // proc.psinfo.maj_flt
    PCP_PROC_CMINFLT,   // proc.psinfo.cmin_flt
    PCP_PROC_CMAJFLT,   // proc.psinfo.cmaj_flt
    PCP_PROC_UTIME,     // proc.psinfo.utime
    PCP_PROC_STIME,     // proc.psinfo.stime
    PCP_PROC_CUTIME,    // proc.psinfo.cutime
    PCP_PROC_CSTIME,    // proc.psinfo.cstime
    PCP_PROC_PRIORITY,  // proc.psinfo.priority
    PCP_PROC_NICE,      // proc.psinfo.nice
    PCP_PROC_THREADS,   // proc.psinfo.threads
    PCP_PROC_STARTTIME, // proc.psinfo.start_time
    PCP_PROC_PROCESSOR, // proc.psinfo.processor
    PCP_PROC_CMD,       // proc.psinfo.cmd
    PCP_PROC_PSARGS,    // proc.psinfo.psargs
    PCP_PROC_CGROUPS,   // proc.psinfo.cgroups
    PCP_PROC_OOMSCORE,  // proc.psinfo.oom_score
    PCP_PROC_VCTXSW,    // proc.psinfo.vctxsw
    PCP_PROC_NVCTXSW,   // proc.psinfo.nvctxsw
    PCP_PROC_LABELS,    // proc.psinfo.labels
    PCP_PROC_ENVIRON,   // proc.psinfo.environ
    PCP_PROC_TTYNAME,   // proc.psinfo.ttyname
    PCP_PROC_EXE,       // proc.psinfo.exe
    PCP_PROC_CWD,       // proc.psinfo.cwd

    PCP_PROC_AUTOGROUP_ID,   // proc.autogroup.id
    PCP_PROC_AUTOGROUP_NICE, // proc.autogroup.nice

    PCP_PROC_ID_UID,  // proc.id.uid
    PCP_PROC_ID_USER, // proc.id.uid_nm

    PCP_PROC_IO_RCHAR,     // proc.io.rchar
    PCP_PROC_IO_WCHAR,     // proc.io.wchar
    PCP_PROC_IO_SYSCR,     // proc.io.syscr
    PCP_PROC_IO_SYSCW,     // proc.io.syscw
    PCP_PROC_IO_READB,     // proc.io.read_bytes
    PCP_PROC_IO_WRITEB,    // proc.io.write_bytes
    PCP_PROC_IO_CANCELLED, // proc.io.cancelled_write_bytes

    PCP_PROC_MEM_SIZE,   // proc.memory.size
    PCP_PROC_MEM_RSS,    // proc.memory.rss
    PCP_PROC_MEM_SHARE,  // proc.memory.share
    PCP_PROC_MEM_TEXTRS, // proc.memory.textrss
    PCP_PROC_MEM_LIBRS,  // proc.memory.librss
    PCP_PROC_MEM_DATRS,  // proc.memory.datrss
    PCP_PROC_MEM_DIRTY,  // proc.memory.dirty

    PCP_PROC_SMAPS_PSS,     // proc.smaps.pss
    PCP_PROC_SMAPS_SWAP,    // proc.smaps.swap
    PCP_PROC_SMAPS_SWAPPSS, // proc.smaps.swappss

    PCP_METRIC_COUNT, // total metric count
}

/// Port of `static inline Metric Metric_fromId(size_t id)` (`pcp/Metric.h:187`)
/// — the C `return (Metric)id`. `#[repr(usize)]` makes the transmute a pure
/// reinterpret (identical to the C cast). Caller guarantees `id < COUNT`.
pub fn Metric_fromId(id: usize) -> Metric {
    unsafe { core::mem::transmute::<usize, Metric>(id) }
}

/// Port of `const pmDesc* Metric_desc(Metric metric)` (`Metric.c:25`).
pub fn Metric_desc(metric: Metric) -> *const pmDesc {
    unsafe {
        let pcp = &mut *platform::pcp.load(Ordering::Relaxed);
        &pcp.descs[metric as usize] as *const pmDesc
    }
}

/// Port of `int Metric_type(Metric metric)` (`Metric.c:29`).
pub fn Metric_type(metric: Metric) -> c_int {
    unsafe {
        let pcp = &mut *platform::pcp.load(Ordering::Relaxed);
        pcp.descs[metric as usize].type_
    }
}

/// Port of `pmAtomValue* Metric_values(Metric, pmAtomValue*, int, int)`
/// (`Metric.c:33`). `atom` is a caller-provided C array of at least `count`
/// entries; `vset->vlist[i]` is flexible-array pointer arithmetic.
pub fn Metric_values(
    metric: Metric,
    atom: *mut pmAtomValue,
    count: c_int,
    type_: c_int,
) -> *mut pmAtomValue {
    unsafe {
        let pcp = &mut *platform::pcp.load(Ordering::Relaxed);
        if pcp.result.is_null() {
            return ptr::null_mut();
        }

        let vset = *(*pcp.result).vset.as_ptr().add(metric as usize);
        if vset.is_null() || (*vset).numval <= 0 {
            return ptr::null_mut();
        }

        // extract requested number of values as requested type
        let desc = &pcp.descs[metric as usize];
        let mut i: c_int = 0;
        while i < (*vset).numval {
            if i == count {
                break;
            }
            let value = &*(*vset).vlist.as_ptr().add(i as usize);
            let sts = pmExtractValue(
                (*vset).valfmt,
                value,
                desc.type_,
                atom.add(i as usize),
                type_,
            );
            if sts < 0 {
                // C logs via pmDebugOptions.appl0 here — debug chatter, omitted.
                *atom.add(i as usize) = core::mem::zeroed();
            }
            i += 1;
        }
        atom
    }
}

/// Port of `int Metric_instanceCount(Metric metric)` (`Metric.c:58`). The C does
/// not guard `pcp->result` here — a null deref is the faithful uninitialized
/// crash.
pub fn Metric_instanceCount(metric: Metric) -> c_int {
    unsafe {
        let pcp = &mut *platform::pcp.load(Ordering::Relaxed);
        let vset = *(*pcp.result).vset.as_ptr().add(metric as usize);
        if !vset.is_null() {
            return (*vset).numval;
        }
        0
    }
}

/// Port of `int Metric_instanceOffset(Metric metric, int inst)` (`Metric.c:65`).
pub fn Metric_instanceOffset(metric: Metric, inst: c_int) -> c_int {
    unsafe {
        let pcp = &mut *platform::pcp.load(Ordering::Relaxed);
        let vset = *(*pcp.result).vset.as_ptr().add(metric as usize);
        if vset.is_null() || (*vset).numval <= 0 {
            return 0;
        }

        // search for optimal offset for subsequent inst lookups to begin
        let mut i: c_int = 0;
        while i < (*vset).numval {
            if inst == (*(*vset).vlist.as_ptr().add(i as usize)).inst {
                return i;
            }
            i += 1;
        }
        0
    }
}

/// Port of the `static pmAtomValue* Metric_extract(...)` helper (`Metric.c:78`)
/// — extract one instance's value (as `type`) into `atom`, publishing the
/// descriptor through `desc`.
#[allow(unused_variables)] // `inst` is used only by the omitted pmDebugOptions.appl0 branch
fn Metric_extract(
    metric: Metric,
    inst: c_int,
    offset: c_int,
    vset: *mut pmValueSet,
    atom: *mut pmAtomValue,
    desc: &mut *const pmDesc,
    type_: c_int,
) -> *mut pmAtomValue {
    unsafe {
        let pcp = &mut *platform::pcp.load(Ordering::Relaxed);
        // extract value (using requested type) of given metric instance
        let outdesc: *const pmDesc = &pcp.descs[metric as usize];
        let value = &*(*vset).vlist.as_ptr().add(offset as usize);
        let sts = pmExtractValue((*vset).valfmt, value, (*outdesc).type_, atom, type_);
        if sts < 0 {
            // C logs via pmDebugOptions.appl0 here — debug chatter, omitted.
            *atom = core::mem::zeroed();
        }
        *desc = outdesc;
        atom
    }
}

/// Port of the `static const pmDesc* Metric_instanceDesc(...)` helper
/// (`Metric.c:94`) — fast-path heuristic offset, else linear scan.
fn Metric_instanceDesc(
    metric: Metric,
    inst: c_int,
    offset: c_int,
    atom: *mut pmAtomValue,
    type_: c_int,
) -> *const pmDesc {
    unsafe {
        let pcp = &mut *platform::pcp.load(Ordering::Relaxed);
        let vset = *(*pcp.result).vset.as_ptr().add(metric as usize);
        if vset.is_null() || (*vset).numval <= 0 {
            return ptr::null();
        }

        // fast-path using heuristic offset based on expected location
        // (Metric_extract always returns the non-null `atom`, so the trailing
        // `&&` mirrors the C's always-true call result)
        let mut desc: *const pmDesc = ptr::null();
        if offset >= 0
            && offset < (*vset).numval
            && inst == (*(*vset).vlist.as_ptr().add(offset as usize)).inst
            && !Metric_extract(metric, inst, offset, vset, atom, &mut desc, type_).is_null()
        {
            return desc;
        }

        // slow-path using a linear search for the requested instance
        let mut i: c_int = 0;
        while i < (*vset).numval {
            if inst == (*(*vset).vlist.as_ptr().add(i as usize)).inst
                && !Metric_extract(metric, inst, i, vset, atom, &mut desc, type_).is_null()
            {
                break;
            }
            i += 1;
        }
        desc
    }
}

/// Port of `pmAtomValue* Metric_instance(...)` (`Metric.c:114`).
pub fn Metric_instance(
    metric: Metric,
    inst: c_int,
    offset: c_int,
    atom: *mut pmAtomValue,
    type_: c_int,
) -> *mut pmAtomValue {
    if !Metric_instanceDesc(metric, inst, offset, atom, type_).is_null() {
        return atom;
    }
    ptr::null_mut()
}

/// Port of `static inline pmAtomValue* kibibytes(pmAtomValue*, int)`
/// (`Metric.c:120`) — integer-math rescale of `atom->ull` by `10^(±N)` in KiB
/// units. Operates on the `ull` union field (unsafe).
fn kibibytes(atom: *mut pmAtomValue, scale: c_int) -> *mut pmAtomValue {
    unsafe {
        // perform integer math, raising to the power +/-N
        let power = scale - PM_SPACE_KBYTE;
        if power > 0 {
            let mut i = 0;
            while i < power {
                (*atom).ull *= ONE_K;
                i += 1;
            }
        } else if power < 0 {
            let mut i = 0;
            while i > power {
                (*atom).ull /= ONE_K;
                i -= 1;
            }
        }
        atom
    }
}

/// Port of `pmAtomValue* Metric_instance_kibibytes(...)` (`Metric.c:133`).
pub fn Metric_instance_kibibytes(
    metric: Metric,
    inst: c_int,
    offset: c_int,
    atom: *mut pmAtomValue,
) -> *mut pmAtomValue {
    unsafe {
        let desc = Metric_instanceDesc(metric, inst, offset, atom, PM_TYPE_U64);
        if !desc.is_null() {
            return kibibytes(atom, (*desc).units.scaleSpace());
        }
        ptr::null_mut()
    }
}

/// Port of `static inline pmAtomValue* milliseconds(pmAtomValue*, int)`
/// (`Metric.c:140`) — rescale `atom->ull` from `scale` time units to msec.
fn milliseconds(atom: *mut pmAtomValue, scale: c_int) -> *mut pmAtomValue {
    unsafe {
        match scale {
            PM_TIME_NSEC => (*atom).ull /= 1_000_000u64,
            PM_TIME_USEC => (*atom).ull /= 1_000u64,
            PM_TIME_MSEC => {}
            PM_TIME_SEC => (*atom).ull *= 1_000u64,
            PM_TIME_MIN => (*atom).ull *= 1_000u64 * 60u64,
            PM_TIME_HOUR => (*atom).ull *= 1_000u64 * 60u64 * 60u64,
            _ => {}
        }
        atom
    }
}

/// Port of `pmAtomValue* Metric_instance_milliseconds(...)` (`Metric.c:163`).
pub fn Metric_instance_milliseconds(
    metric: Metric,
    inst: c_int,
    offset: c_int,
    atom: *mut pmAtomValue,
) -> *mut pmAtomValue {
    unsafe {
        let desc = Metric_instanceDesc(metric, inst, offset, atom, PM_TYPE_U64);
        if !desc.is_null() {
            return milliseconds(atom, (*desc).units.scaleTime());
        }
        ptr::null_mut()
    }
}

/// Port of `bool Metric_iterate(Metric, int*, int*, size_t)` (`Metric.c:176`).
/// Iterate over a set of instances (incl `PM_IN_NULL`), returning the next
/// instance identifier and offset. Start by passing `offset = -1`.
pub fn Metric_iterate(
    metric: Metric,
    instp: &mut c_int,
    offsetp: &mut c_int,
    entrylen: usize,
) -> bool {
    unsafe {
        let pcp = &mut *platform::pcp.load(Ordering::Relaxed);
        if pcp.result.is_null() {
            return false;
        }

        let vset = *(*pcp.result).vset.as_ptr().add(metric as usize);
        if vset.is_null()
            || (*vset).numval <= 0
            || (*vset).numval as usize > c_long::MAX as usize / entrylen
        {
            return false;
        }

        let mut offset = *offsetp;
        offset = if offset < 0 { 0 } else { offset + 1 };
        if offset > (*vset).numval - 1 {
            return false;
        }

        *offsetp = offset;
        *instp = (*(*vset).vlist.as_ptr().add(offset as usize)).inst;
        true
    }
}

/// Port of `void Metric_enable(Metric metric, bool enable)` (`Metric.c:195`) —
/// switch on/off a metric for value fetching (sampling).
pub fn Metric_enable(metric: Metric, enable: bool) {
    unsafe {
        let pcp = &mut *platform::pcp.load(Ordering::Relaxed);
        pcp.fetch[metric as usize] = if enable {
            pcp.pmids[metric as usize]
        } else {
            PM_ID_NULL
        };
    }
}

/// Port of `bool Metric_enabled(Metric metric)` (`Metric.c:199`).
pub fn Metric_enabled(metric: Metric) -> bool {
    unsafe {
        let pcp = &mut *platform::pcp.load(Ordering::Relaxed);
        pcp.fetch[metric as usize] != PM_ID_NULL
    }
}

/// Port of `void Metric_enableThreads(void)` (`Metric.c:203`). The C
/// `xCalloc`s a single-instance `pmValueSet`/`pmResult`, `pmStore`s it, then
/// `pmFreeResult`s it. Here the two are `Box`-allocated (zeroed, matching the
/// C's calloc) and handed to libpcp as raw pointers; `pmFreeResult` (libpcp's
/// `free()`) then owns them, mirroring the C free path exactly.
///
/// Deviation note: the blocks are `Box`-allocated (Rust global allocator) yet
/// freed by libpcp's `pmFreeResult`/`free`. On the targets where Rust's global
/// allocator is the system `malloc` (the default on macOS/Linux) this is
/// identical to the C `xCalloc`+`free` pairing; it is the faithful mirror of the
/// C call sequence with no owned copy left on the Rust side.
pub fn Metric_enableThreads() {
    unsafe {
        let pcp = &mut *platform::pcp.load(Ordering::Relaxed);
        let vset = Box::into_raw(Box::new(pmValueSet {
            pmid: pcp.pmids[Metric::PCP_CONTROL_THREADS as usize],
            numval: 1,
            valfmt: PM_VAL_INSITU,
            vlist: [pmValue {
                inst: PM_IN_NULL,
                value: pmValue_value { lval: 1 },
            }],
        }));

        let result = Box::into_raw(Box::new(pmResult {
            timestamp: core::mem::zeroed(),
            numpmid: 1,
            vset: [vset],
        }));

        let sts = pmStore(result);
        if sts < 0 {
            // C logs via pmDebugOptions.appl0 here — debug chatter, omitted.
        }

        pmFreeResult(result);
    }
}

/// Port of `bool Metric_fetch(struct timeval* timestamp)` (`Metric.c:222`).
/// `timestamp` is optional (`NULL` in C). The `#if PMAPI_VERSION >= 3` timestamp
/// branch is inlined (nsec→usec) to avoid the `pmtimespecTotimeval` extern.
pub fn Metric_fetch(timestamp: Option<&mut libc::timeval>) -> bool {
    unsafe {
        let pcp = &mut *platform::pcp.load(Ordering::Relaxed);
        if !pcp.result.is_null() {
            pmFreeResult(pcp.result);
            pcp.result = ptr::null_mut();
        }
        if pcp.reconnect {
            if pmReconnectContext(pcp.context) < 0 {
                return false;
            }
            pcp.reconnect = false;
        }

        let mut sts: c_int;
        let mut count: c_int = 0;
        let total = pcp.totalMetrics as c_int;
        loop {
            sts = pmFetch(total, pcp.fetch.as_mut_ptr(), &mut pcp.result);
            count += 1;
            if !(sts == PM_ERR_IPC && count < 3) {
                break;
            }
        }
        if sts < 0 {
            // C logs via pmDebugOptions.appl0 here — debug chatter, omitted.
            pcp.reconnect = true;
            return false;
        }
        if let Some(ts) = timestamp {
            // #if PMAPI_VERSION >= 3 branch, inlined (pmtimespecTotimeval)
            ts.tv_sec = (*pcp.result).timestamp.tv_sec;
            ts.tv_usec = ((*pcp.result).timestamp.tv_nsec / 1000) as _;
        }
        true
    }
}

/// Port of `void Metric_externalName(Metric, int, char**)` (`Metric.c:254`).
/// The `pmNameInDom` failure is ignored, exactly as the C `(void)` cast does.
pub fn Metric_externalName(metric: Metric, inst: c_int, external_name: &mut *mut c_char) {
    unsafe {
        let pcp = &mut *platform::pcp.load(Ordering::Relaxed);
        let desc = &pcp.descs[metric as usize];
        // ignore a failure here - its safe to do so
        let _ = pmNameInDom(desc.indom, inst, external_name as *mut *mut c_char);
    }
}

/// Port of `int Metric_lookupText(const char* metric, char** desc)`
/// (`Metric.c:260`). Uppercases the first char of the one-line help text for UI
/// consistency (the C `toupper` on ASCII).
pub fn Metric_lookupText(metric: &CStr, desc: &mut *mut c_char) -> c_int {
    unsafe {
        let mut pmid: pmID = 0;
        let namelist: *const c_char = metric.as_ptr();
        let sts = pmLookupName(1, &namelist as *const *const c_char, &mut pmid);
        if sts < 0 {
            return sts;
        }

        if pmLookupText(pmid, PM_TEXT_ONELINE, desc as *mut *mut c_char) >= 0 {
            // (*desc)[0] = toupper((*desc)[0]); /* UI consistency */
            let s = *desc;
            *s = (*s as u8).to_ascii_uppercase() as c_char;
        }
        0
    }
}
