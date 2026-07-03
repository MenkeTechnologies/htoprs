//! Port of `LinuxMachine.c` — the Linux implementation of htop's per-host
//! `Machine`: CPU counts/topology, `/proc/meminfo`, huge pages, ZRAM,
//! ZFS ARC, `/proc/stat` CPU time and CPU frequency scanning.
//!
//! C names are preserved verbatim (`CamelCase_snake`), so `non_snake_case`
//! is allowed for the whole module.
//!
//! # Struct model (substrate owned by this file)
//!
//! This file owns the `LinuxMachine` struct and its satellite POD types
//! (`CPUData`, `GPUEngineData`) declared in `linux/LinuxMachine.h`, plus
//! the small data-only stats structs (`ZfsArcStats`, `ZramStats`,
//! `ZswapStats`) that `LinuxMachine.h` embeds by value and whose own
//! headers (`zfs/ZfsArcStats.h`, `linux/ZramStats.h`, `linux/ZswapStats.h`)
//! declare no functions. They are modeled here in full so the `linux/`
//! scan layer and the ZRAM/ZFS/huge-page meters can read them.
//!
//! `LinuxMachine` embeds the base `Machine` as `super_` (htop's
//! `Machine super;` first member); the C `(LinuxMachine*)super` upcast is
//! a `&LinuxMachine` in Rust.
//!
//! # Sensors build variant
//!
//! `HAVE_SENSORS_SENSORS_H` is **not** defined for this port (LibSensors
//! is stubbed), so per the port rules the `#ifdef`-gated sensors branches
//! — `CPUData.temperature`, `LibSensors_reload`/`_getCPUTemperatures`/
//! `_countCCDs`, and `Machine_scan`'s `showCPUTemperature` clause — are
//! omitted, keeping the module on the no-sensors branch it committed to.
//!
//! # File I/O
//!
//! htop reads sysfs/procfs via `fopen`/`Compat_readfile*`/`opendir`; those
//! Compat helpers are still stubbed, so the ports read the same paths
//! directly with `std::fs` (the established idiom in this tree, e.g.
//! `openfilesscreen.rs`). Behaviour is matched byte-for-byte where it
//! affects results.
#![allow(non_snake_case)]
#![allow(non_camel_case_types)]
#![allow(dead_code)]

use std::fs;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::Instant;

use crate::ported::crt::CRT_fatalError;
use crate::ported::machine::Machine;
use crate::ported::xutils::String_startsWith;

/// `typedef unsigned long long int memory_t` (`Machine.h:35`).
pub type memory_t = u64;
/// `#define MEMORY_MAX ULLONG_MAX` (`Machine.h:36`).
pub const MEMORY_MAX: memory_t = u64::MAX;

/// `#define HTOP_HUGEPAGE_BASE_SHIFT 16` (`LinuxMachine.h:18`).
pub const HTOP_HUGEPAGE_BASE_SHIFT: usize = 16;
/// `#define HTOP_HUGEPAGE_COUNT 24` (`LinuxMachine.h:19`).
pub const HTOP_HUGEPAGE_COUNT: usize = 24;

/// `#define PROCCPUINFOFILE PROCDIR "/cpuinfo"` (`LinuxMachine.h:109`).
const PROCCPUINFOFILE: &str = "/proc/cpuinfo";
/// `#define PROCSTATFILE PROCDIR "/stat"` (`LinuxMachine.h:113`).
const PROCSTATFILE: &str = "/proc/stat";
/// `#define PROCMEMINFOFILE PROCDIR "/meminfo"` (`LinuxMachine.h:117`).
const PROCMEMINFOFILE: &str = "/proc/meminfo";
/// `#define PROCARCSTATSFILE PROCDIR "/spl/kstat/zfs/arcstats"` (`LinuxMachine.h:121`).
const PROCARCSTATSFILE: &str = "/proc/spl/kstat/zfs/arcstats";
/// `#define PROC_LINE_LENGTH 4096` (`LinuxMachine.h:129`).
const PROC_LINE_LENGTH: usize = 4096;

/// Port of `typedef struct ZfsArcStats_` (`zfs/ZfsArcStats.h:12`). All
/// sizes are in kB after `LinuxMachine_scanZfsArcstats` post-processing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ZfsArcStats {
    pub enabled: i32,
    pub isCompressed: i32,
    pub min: memory_t,
    pub max: memory_t,
    pub size: memory_t,
    pub MFU: memory_t,
    pub MRU: memory_t,
    pub anon: memory_t,
    pub header: memory_t,
    pub other: memory_t,
    pub compressed: memory_t,
    pub uncompressed: memory_t,
}

/// Port of `typedef struct ZramStats_` (`linux/ZramStats.h:12`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ZramStats {
    pub totalZram: memory_t,
    pub usedZramComp: memory_t,
    pub usedZramOrig: memory_t,
}

/// Port of `typedef struct ZswapStats_` (`linux/ZswapStats.h:12`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ZswapStats {
    /// amount of RAM used by the zswap pool
    pub usedZswapComp: memory_t,
    /// amount of data stored inside the zswap pool
    pub usedZswapOrig: memory_t,
}

/// Port of `typedef struct CPUData_` (`LinuxMachine.h:21`). Index 0 of
/// `LinuxMachine::cpuData` is the aggregate ("average") CPU; indices
/// `1..=existingCPUs` are the physical threads.
///
/// The `#ifdef HAVE_SENSORS_SENSORS_H double temperature` field is omitted
/// (no-sensors build variant; see module docs).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CPUData {
    pub totalTime: u64,
    pub userTime: u64,
    pub systemTime: u64,
    pub systemAllTime: u64,
    pub idleAllTime: u64,
    pub idleTime: u64,
    pub niceTime: u64,
    pub ioWaitTime: u64,
    pub irqTime: u64,
    pub softIrqTime: u64,
    pub stealTime: u64,
    pub guestTime: u64,

    pub totalPeriod: u64,
    pub userPeriod: u64,
    pub systemPeriod: u64,
    pub systemAllPeriod: u64,
    pub idleAllPeriod: u64,
    pub idlePeriod: u64,
    pub nicePeriod: u64,
    pub ioWaitPeriod: u64,
    pub irqPeriod: u64,
    pub softIrqPeriod: u64,
    pub stealPeriod: u64,
    pub guestPeriod: u64,

    pub frequency: f64,

    /// different for each CPU socket
    pub physicalID: i32,
    /// same for hyperthreading
    pub coreID: i32,
    /// same for each AMD chiplet
    pub ccdID: i32,
    /// Normalized physical core ID
    pub coreIndex: i32,
    /// SMT thread index: 0 for first thread, 1 for second, etc.
    pub threadIndex: i32,

    pub online: bool,
}

/// Port of `typedef struct GPUEngineData_` (`LinuxMachine.h:63`). A
/// singly-linked list of per-engine GPU busy times; `next` is the C
/// `struct GPUEngineData_*` link.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GPUEngineData {
    /// absolute GPU time in nano seconds (previous sample)
    pub prevTime: u64,
    /// absolute GPU time in nano seconds (current sample)
    pub curTime: u64,
    /// engine name (C `char* key`)
    pub key: Option<String>,
    pub next: Option<Box<GPUEngineData>>,
}

/// Port of `typedef struct LinuxMachine_` (`LinuxMachine.h:69`). Embeds
/// the base [`Machine`] as `super_` (C's `Machine super;`).
///
/// `#[repr(C)]` keeps `super_` at offset 0 so the C `(LinuxMachine*)host`
/// downcast — a `*const Machine` obtained from a `LinuxMachine`, cast back —
/// is sound (used by the linux platform meter value-setters).
#[repr(C)]
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LinuxMachine {
    pub super_: Machine,

    pub jiffies: i64,
    pub pageSize: usize,
    pub pageSizeKB: usize,

    /// `procs_running` from `/proc/stat`
    pub runningTasks: u32,
    /// `btime` field from `/proc/stat`
    pub boottime: i64,

    pub period: f64,

    pub cachedMem: memory_t,
    pub sharedMem: memory_t,
    pub usedMem: memory_t,
    pub buffersMem: memory_t,
    pub availableMem: memory_t,

    /// index 0 == aggregate; `1..=existingCPUs` == physical threads
    pub cpuData: Vec<CPUData>,

    pub maxPhysicalID: i32,
    pub maxCoreID: i32,

    pub totalHugePageMem: memory_t,
    pub usedHugePageMem: [memory_t; HTOP_HUGEPAGE_COUNT],

    /// total absolute GPU time in nano seconds (previous sample)
    pub prevGpuTime: u64,
    /// total absolute GPU time in nano seconds (current sample)
    pub curGpuTime: u64,
    pub gpuEngineData: Option<Box<GPUEngineData>>,

    pub zfs: ZfsArcStats,
    pub zram: ZramStats,
    pub zswap: ZswapStats,
}

/// Port of `static void LinuxMachine_updateCPUcount(LinuxMachine* this)`
/// from `LinuxMachine.c:47`. Enumerates `/sys/devices/system/cpu/cpuN`
/// entries, growing `cpuData` (index 0 is the always-online aggregate) and
/// counting online/existing CPUs. `Compat_readfileat(cpuDirFd, "online")`
/// is read directly from the `.../cpuN/online` path.
///
/// The `HAVE_SENSORS_SENSORS_H` `LibSensors_reload()` reload-on-online
/// clause is omitted (no-sensors build variant; see module docs).
fn LinuxMachine_updateCPUcount(this: &mut LinuxMachine) {
    let mut existing: u32 = 0;
    let mut active: u32 = 0;

    // Initialize the cpuData array before anything else.
    if this.cpuData.is_empty() {
        this.cpuData = vec![CPUData::default(); 2];
        this.cpuData[0].online = true; /* average is always "online" */
        this.cpuData[1].online = true;
        this.super_.activeCPUs = 1;
        this.super_.existingCPUs = 1;
    }

    let dir = match fs::read_dir("/sys/devices/system/cpu") {
        Ok(d) => d,
        Err(_) => return,
    };

    let mut currExisting: u32 = this.super_.existingCPUs;

    for entry in dir.flatten() {
        // if (entry->d_type != DT_DIR && entry->d_type != DT_UNKNOWN) continue;
        match entry.file_type() {
            Ok(ft) if !ft.is_dir() => continue,
            _ => {}
        }

        let name = entry.file_name();
        let name = match name.to_str() {
            Some(n) => n,
            None => continue,
        };

        if !String_startsWith(name, "cpu") {
            continue;
        }

        // strtoul(name + 3): the suffix must be a non-empty run of digits.
        let suffix = &name[3..];
        if suffix.is_empty() || !suffix.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let sysid: u64 = match suffix.parse() {
            Ok(v) => v,
            Err(_) => continue,
        };
        if sysid >= u32::MAX as u64 {
            continue;
        }
        let cpuid: u32 = sysid as u32 + 1;

        existing += 1;

        /* readdir() iterates with no specific order */
        let max = existing.max(cpuid);
        if max > currExisting {
            this.cpuData
                .resize(max as usize + /* aggregate */ 1, CPUData::default());
            this.cpuData[0].online = true; /* average is always "online" */
            currExisting = max;
        }

        // Compat_readfileat(cpuDirFd, "online", ...): res < 1 or first byte
        // != '0' counts as online/active.
        let online_path = format!("/sys/devices/system/cpu/{}/online", name);
        let online = match fs::read(&online_path) {
            Ok(buf) if !buf.is_empty() => buf[0] != b'0',
            _ => true,
        };
        if online {
            active += 1;
            this.cpuData[cpuid as usize].online = true;
        } else {
            this.cpuData[cpuid as usize].online = false;
        }
    }

    // return if no CPU is found
    if existing < 1 {
        return;
    }

    this.super_.activeCPUs = active;
    debug_assert_eq!(existing, currExisting);
    this.super_.existingCPUs = currExisting;
}

/// Port of `static void LinuxMachine_scanMemoryInfo(LinuxMachine* this)`
/// from `LinuxMachine.c:130`. Parses `/proc/meminfo` and computes the
/// procps-style memory partition.
fn LinuxMachine_scanMemoryInfo(this: &mut LinuxMachine) {
    let mut availableMem: memory_t = 0;
    let mut freeMem: memory_t = 0;
    let mut totalMem: memory_t = 0;
    let mut buffersMem: memory_t = 0;
    let mut cachedMem: memory_t = 0;
    let mut sharedMem: memory_t = 0;
    let mut swapTotalMem: memory_t = 0;
    let mut swapCacheMem: memory_t = 0;
    let mut swapFreeMem: memory_t = 0;
    let mut sreclaimableMem: memory_t = 0;
    let mut zswapCompMem: memory_t = 0;
    let mut zswapOrigMem: memory_t = 0;

    let content = match fs::read_to_string(PROCMEMINFOFILE) {
        Ok(c) => c,
        Err(_) => CRT_fatalError("Cannot open /proc/meminfo"),
    };

    // tryRead(label, var): if line starts with label, sscanf "%llu kB".
    let try_read = |line: &str, label: &str| -> Option<memory_t> {
        if String_startsWith(line, label) {
            line[label.len()..]
                .split_whitespace()
                .next()
                .and_then(|t| t.parse::<memory_t>().ok())
        } else {
            None
        }
    };

    for line in content.lines() {
        if let Some(v) = try_read(line, "MemAvailable:") {
            availableMem = v;
        } else if let Some(v) = try_read(line, "MemFree:") {
            freeMem = v;
        } else if let Some(v) = try_read(line, "MemTotal:") {
            totalMem = v;
        } else if let Some(v) = try_read(line, "Buffers:") {
            buffersMem = v;
        } else if let Some(v) = try_read(line, "Cached:") {
            cachedMem = v;
        } else if let Some(v) = try_read(line, "Shmem:") {
            sharedMem = v;
        } else if let Some(v) = try_read(line, "SwapTotal:") {
            swapTotalMem = v;
        } else if let Some(v) = try_read(line, "SwapCached:") {
            swapCacheMem = v;
        } else if let Some(v) = try_read(line, "SwapFree:") {
            swapFreeMem = v;
        } else if let Some(v) = try_read(line, "SReclaimable:") {
            sreclaimableMem = v;
        } else if let Some(v) = try_read(line, "Zswap:") {
            zswapCompMem = v;
        } else if let Some(v) = try_read(line, "Zswapped:") {
            zswapOrigMem = v;
        }
    }

    /*
     * Compute memory partition like procps(free); Shmem is part of Cached.
     */
    this.super_.totalMem = totalMem;
    this.cachedMem = cachedMem
        .wrapping_add(sreclaimableMem)
        .wrapping_sub(sharedMem);
    this.sharedMem = sharedMem;
    let usedDiff: memory_t = freeMem + cachedMem + sreclaimableMem + buffersMem;
    this.usedMem = if totalMem >= usedDiff {
        totalMem - usedDiff
    } else {
        totalMem.wrapping_sub(freeMem)
    };
    this.buffersMem = buffersMem;
    this.availableMem = if availableMem != 0 {
        availableMem.min(totalMem)
    } else {
        freeMem
    };
    this.super_.totalSwap = swapTotalMem;
    this.super_.usedSwap = swapTotalMem
        .wrapping_sub(swapFreeMem)
        .wrapping_sub(swapCacheMem);
    this.super_.cachedSwap = swapCacheMem;
    this.zswap.usedZswapComp = zswapCompMem;
    this.zswap.usedZswapOrig = zswapOrigMem;
}

/// Port of `static void LinuxMachine_scanHugePages(LinuxMachine* this)`
/// from `LinuxMachine.c:221`. Sums huge-page usage per page size from
/// `/sys/kernel/mm/hugepages/hugepages-<size>kB/{nr,free}_hugepages`.
fn LinuxMachine_scanHugePages(this: &mut LinuxMachine) {
    this.totalHugePageMem = 0;
    for i in 0..HTOP_HUGEPAGE_COUNT {
        this.usedHugePageMem[i] = MEMORY_MAX;
    }

    let dir = match fs::read_dir("/sys/kernel/mm/hugepages") {
        Ok(d) => d,
        Err(_) => return,
    };

    for entry in dir.flatten() {
        /* Ignore all non-directories */
        match entry.file_type() {
            Ok(ft) if !ft.is_dir() => continue,
            _ => {}
        }

        let name = entry.file_name();
        let name = match name.to_str() {
            Some(n) => n,
            None => continue,
        };

        if !String_startsWith(name, "hugepages-") {
            continue;
        }

        // strtoul(name + strlen("hugepages-")); endptr must point at 'k'.
        let rest = &name["hugepages-".len()..];
        let digits: String = rest
            .bytes()
            .take_while(|b| b.is_ascii_digit())
            .map(|b| b as char)
            .collect();
        let hugePageSize: u64 = match digits.parse() {
            Ok(v) => v,
            Err(_) => continue,
        };
        if rest.as_bytes().get(digits.len()) != Some(&b'k') {
            continue;
        }

        let nr_path = format!("/sys/kernel/mm/hugepages/{}/nr_hugepages", name);
        let total: memory_t = match fs::read_to_string(&nr_path) {
            Ok(s) if !s.trim().is_empty() => s.trim().parse().unwrap_or(0),
            _ => continue,
        };
        if total == 0 {
            continue;
        }

        let free_path = format!("/sys/kernel/mm/hugepages/{}/free_hugepages", name);
        let free: memory_t = match fs::read_to_string(&free_path) {
            Ok(s) if !s.trim().is_empty() => s.trim().parse().unwrap_or(0),
            _ => continue,
        };

        // ffsl(x) == trailing_zeros(x) + 1 for x != 0.
        let ffsl = hugePageSize.trailing_zeros() as i64 + 1;
        let shift = ffsl - 1 - (HTOP_HUGEPAGE_BASE_SHIFT as i64 - 10);
        debug_assert!(shift >= 0 && shift < HTOP_HUGEPAGE_COUNT as i64);

        this.totalHugePageMem += total * hugePageSize;
        this.usedHugePageMem[shift as usize] = (total - free) * hugePageSize;
    }
}

/// Port of `static void LinuxMachine_scanZramInfo(LinuxMachine* this)`
/// from `LinuxMachine.c:323`. Enumerates `zramN` block devices under
/// `/sys/block` and sums their disk/compressed/original sizes.
///
/// The C static helpers `LinuxMachine_isZramBlockName` (`LinuxMachine.c:277`)
/// and `LinuxMachine_scanZramDevice` (`LinuxMachine.c:293`) are inlined as
/// closures (they are not free C symbols in the port-name index).
fn LinuxMachine_scanZramInfo(this: &mut LinuxMachine) {
    let mut totalZram: memory_t = 0;
    let mut usedZramComp: memory_t = 0;
    let mut usedZramOrig: memory_t = 0;

    // LinuxMachine_isZramBlockName: "zram" followed by >=1 digits only.
    let is_zram_block_name = |name: &str| -> bool {
        match name.strip_prefix("zram") {
            Some(rest) => !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()),
            None => false,
        }
    };

    if let Ok(dir) = fs::read_dir("/sys/block") {
        for entry in dir.flatten() {
            let name = entry.file_name();
            let name = match name.to_str() {
                Some(n) => n,
                None => continue,
            };

            /* zram devices are named zramN */
            if !is_zram_block_name(name) {
                continue;
            }

            // LinuxMachine_scanZramDevice: read disksize + mm_stat.
            let disksize = fs::read_to_string(format!("/sys/block/{}/disksize", name));
            let mm_stat = fs::read_to_string(format!("/sys/block/{}/mm_stat", name));
            if let (Ok(disksize), Ok(mm_stat)) = (disksize, mm_stat) {
                let size: Option<memory_t> = disksize
                    .split_whitespace()
                    .next()
                    .and_then(|t| t.parse().ok());
                let mut mm = mm_stat.split_whitespace();
                let orig_data_size: Option<memory_t> = mm.next().and_then(|t| t.parse().ok());
                let compr_data_size: Option<memory_t> = mm.next().and_then(|t| t.parse().ok());
                if let (Some(size), Some(orig), Some(compr)) =
                    (size, orig_data_size, compr_data_size)
                {
                    totalZram += size;
                    usedZramComp += compr;
                    usedZramOrig += orig;
                }
            }
        }
    }

    this.zram.totalZram = totalZram / 1024;
    this.zram.usedZramComp = usedZramComp / 1024;
    this.zram.usedZramOrig = usedZramOrig / 1024;
    if this.zram.usedZramComp > this.zram.usedZramOrig {
        this.zram.usedZramComp = this.zram.usedZramOrig;
    }
}

/// Port of `static void LinuxMachine_scanZfsArcstats(LinuxMachine* this)`
/// from `LinuxMachine.c:356`. Parses `/proc/spl/kstat/zfs/arcstats`
/// (three whitespace columns: name, type, value) into `this.zfs`.
fn LinuxMachine_scanZfsArcstats(this: &mut LinuxMachine) {
    let mut dbufSize: memory_t = 0;
    let mut dnodeSize: memory_t = 0;
    let mut bonusSize: memory_t = 0;

    let content = match fs::read_to_string(PROCARCSTATSFILE) {
        Ok(c) => c,
        Err(_) => {
            this.zfs.enabled = 0;
            return;
        }
    };

    // tryRead(label, var): sscanf(buffer + strlen(label), " %*2u %32llu"):
    // skip the 2-digit type column, take the value column.
    let try_read = |line: &str, label: &str| -> Option<memory_t> {
        if String_startsWith(line, label) {
            line[label.len()..]
                .split_whitespace()
                .nth(1)
                .and_then(|t| t.parse::<memory_t>().ok())
        } else {
            None
        }
    };

    for line in content.lines() {
        if let Some(v) = try_read(line, "c_min") {
            this.zfs.min = v;
        } else if let Some(v) = try_read(line, "c_max") {
            this.zfs.max = v;
        } else if String_startsWith(line, "compressed_size") {
            // tryReadFlag: isCompressed = whether the value parsed.
            match try_read(line, "compressed_size") {
                Some(v) => {
                    this.zfs.compressed = v;
                    this.zfs.isCompressed = 1;
                }
                None => this.zfs.isCompressed = 0,
            }
        } else if let Some(v) = try_read(line, "uncompressed_size") {
            this.zfs.uncompressed = v;
        } else if let Some(v) = try_read(line, "size") {
            this.zfs.size = v;
        } else if let Some(v) = try_read(line, "hdr_size") {
            this.zfs.header = v;
        } else if let Some(v) = try_read(line, "dbuf_size") {
            dbufSize = v;
        } else if let Some(v) = try_read(line, "dnode_size") {
            dnodeSize = v;
        } else if let Some(v) = try_read(line, "bonus_size") {
            bonusSize = v;
        } else if let Some(v) = try_read(line, "anon_size") {
            this.zfs.anon = v;
        } else if let Some(v) = try_read(line, "mfu_size") {
            this.zfs.MFU = v;
        } else if let Some(v) = try_read(line, "mru_size") {
            this.zfs.MRU = v;
        }
    }

    this.zfs.enabled = if this.zfs.size > 0 { 1 } else { 0 };
    this.zfs.size /= 1024;
    this.zfs.min /= 1024;
    this.zfs.max /= 1024;
    this.zfs.MFU /= 1024;
    this.zfs.MRU /= 1024;
    this.zfs.anon /= 1024;
    this.zfs.header /= 1024;
    this.zfs.other = (dbufSize + dnodeSize + bonusSize) / 1024;
    if this.zfs.isCompressed != 0 {
        this.zfs.compressed /= 1024;
        this.zfs.uncompressed /= 1024;
    }
}

/// Port of `static void LinuxMachine_scanCPUTime(LinuxMachine* this)` from
/// `LinuxMachine.c:430`. Reads `/proc/stat`, deriving per-CPU period/time
/// counters (via `saturatingSub`) and `runningTasks`. `saturatingSub` is
/// inlined as `a.saturating_sub(b)`.
fn LinuxMachine_scanCPUTime(this: &mut LinuxMachine) {
    LinuxMachine_updateCPUcount(this);

    let existingCPUs = this.super_.existingCPUs;
    let activeCPUs = this.super_.activeCPUs;

    let file = match File::open(PROCSTATFILE) {
        Ok(f) => f,
        Err(_) => CRT_fatalError("Cannot open /proc/stat"),
    };
    let mut reader = BufReader::new(file);

    // One thread per CPU thread + one for the average
    debug_assert!(existingCPUs < u32::MAX - 1);
    let mut adjCpuIdProcessed = vec![false; existingCPUs as usize + 1];

    let mut buffer = String::new();
    for i in 0..=existingCPUs {
        buffer.clear();
        let n = reader.read_line(&mut buffer).unwrap_or(0);
        if n == 0 {
            break;
        }
        let line = buffer.trim_end_matches('\n');

        // cpu fields are sorted first
        if !String_startsWith(line, "cpu") {
            break;
        }

        // Depending on the kernel, 5/7/8/9 fields will be set; the rest 0.
        let after = &line[3..];
        let adjCpuId: u32;
        let mut nums = [0u64; 10];
        if i == 0 {
            // "cpu  %llu %llu ..."
            for (slot, tok) in nums.iter_mut().zip(after.split_whitespace()) {
                *slot = tok.parse().unwrap_or(0);
            }
            adjCpuId = 0;
        } else {
            // "cpu%u %llu %llu ..."
            let mut toks = after.split_whitespace();
            let cpuid: u32 = match toks.next().and_then(|t| t.parse().ok()) {
                Some(v) => v,
                None => break,
            };
            if cpuid >= existingCPUs {
                break;
            }
            for (slot, tok) in nums.iter_mut().zip(toks) {
                *slot = tok.parse().unwrap_or(0);
            }
            adjCpuId = cpuid + 1;
        }

        if adjCpuId > existingCPUs {
            break;
        }

        let mut usertime = nums[0];
        let mut nicetime = nums[1];
        let systemtime = nums[2];
        let idletime = nums[3];
        let ioWait = nums[4];
        let irq = nums[5];
        let softIrq = nums[6];
        let steal = nums[7];
        let guest = nums[8];
        let guestnice = nums[9];

        // Guest time is already accounted in usertime
        usertime = usertime.wrapping_sub(guest);
        nicetime = nicetime.wrapping_sub(guestnice);
        let idlealltime = idletime + ioWait;
        let systemalltime = systemtime + irq + softIrq;
        let virtalltime = guest + guestnice;
        let totaltime = usertime + nicetime + systemalltime + idlealltime + steal + virtalltime;

        let cpuData = &mut this.cpuData[adjCpuId as usize];
        cpuData.userPeriod = usertime.saturating_sub(cpuData.userTime);
        cpuData.nicePeriod = nicetime.saturating_sub(cpuData.niceTime);
        cpuData.systemPeriod = systemtime.saturating_sub(cpuData.systemTime);
        cpuData.systemAllPeriod = systemalltime.saturating_sub(cpuData.systemAllTime);
        cpuData.idleAllPeriod = idlealltime.saturating_sub(cpuData.idleAllTime);
        cpuData.idlePeriod = idletime.saturating_sub(cpuData.idleTime);
        cpuData.ioWaitPeriod = ioWait.saturating_sub(cpuData.ioWaitTime);
        cpuData.irqPeriod = irq.saturating_sub(cpuData.irqTime);
        cpuData.softIrqPeriod = softIrq.saturating_sub(cpuData.softIrqTime);
        cpuData.stealPeriod = steal.saturating_sub(cpuData.stealTime);
        cpuData.guestPeriod = virtalltime.saturating_sub(cpuData.guestTime);
        cpuData.totalPeriod = totaltime.saturating_sub(cpuData.totalTime);
        cpuData.userTime = usertime;
        cpuData.niceTime = nicetime;
        cpuData.systemTime = systemtime;
        cpuData.systemAllTime = systemalltime;
        cpuData.idleAllTime = idlealltime;
        cpuData.idleTime = idletime;
        cpuData.ioWaitTime = ioWait;
        cpuData.irqTime = irq;
        cpuData.softIrqTime = softIrq;
        cpuData.stealTime = steal;
        cpuData.guestTime = virtalltime;
        cpuData.totalTime = totaltime;

        adjCpuIdProcessed[adjCpuId as usize] = true;
    }

    for i in 0..=existingCPUs as usize {
        if !adjCpuIdProcessed[i] {
            // Skipped an ID => thread is offline; /proc/stat is ordered.
            this.cpuData[i] = CPUData::default();
        }
    }

    this.period = this.cpuData[0].totalPeriod as f64 / activeCPUs as f64;

    // Continue reading remaining lines to find procs_running.
    loop {
        buffer.clear();
        let n = reader.read_line(&mut buffer).unwrap_or(0);
        if n == 0 {
            break;
        }
        if String_startsWith(&buffer, "procs_running") {
            this.runningTasks = buffer["procs_running".len()..]
                .split_whitespace()
                .next()
                .and_then(|t| t.parse().ok())
                .unwrap_or(0);
            break;
        }
    }
}

/// Port of `static int scanCPUFrequencyFromSysCPUFreq(LinuxMachine* this)`
/// from `LinuxMachine.c:538`. Reads `scaling_cur_freq` per online CPU from
/// sysfs, converting kHz to MHz. Returns 0 on success, -1 when timed out /
/// bailed early (slow first read), or `-errno` when a file cannot be
/// opened. The static `timeout` counter becomes an [`AtomicI32`].
fn scanCPUFrequencyFromSysCPUFreq(this: &mut LinuxMachine) -> i32 {
    static TIMEOUT: AtomicI32 = AtomicI32::new(0);

    let existingCPUs = this.super_.existingCPUs;
    let mut numCPUsWithFrequency = 0;
    let mut totalFrequency: u64 = 0;

    if TIMEOUT.load(Ordering::Relaxed) > 0 {
        TIMEOUT.fetch_sub(1, Ordering::Relaxed);
        return -1;
    }

    for i in 0..existingCPUs {
        if !Machine_isCPUonline(this, i) {
            continue;
        }

        let path = format!("/sys/devices/system/cpu/cpu{}/cpufreq/scaling_cur_freq", i);

        let start = if i == 0 { Some(Instant::now()) } else { None };

        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => return -(e.raw_os_error().unwrap_or(1)),
        };

        if let Some(frequency) = content
            .split_whitespace()
            .next()
            .and_then(|t| t.parse::<u64>().ok())
        {
            /* convert kHz to MHz */
            let frequency = frequency / 1000;
            this.cpuData[i as usize + 1].frequency = frequency as f64;
            numCPUsWithFrequency += 1;
            totalFrequency += frequency;
        }

        if let Some(start) = start {
            let timeTakenUs = start.elapsed().as_micros();
            if timeTakenUs > 500 {
                TIMEOUT.store(30, Ordering::Relaxed);
                return -1;
            }
        }
    }

    if numCPUsWithFrequency > 0 {
        this.cpuData[0].frequency = totalFrequency as f64 / numCPUsWithFrequency as f64;
    }

    0
}

/// Port of `static void scanCPUFrequencyFromCPUinfo(LinuxMachine* this)`
/// from `LinuxMachine.c:600`. Falls back to `/proc/cpuinfo` per-CPU MHz
/// fields; sysfs data already present is not overridden
/// (`isNonnegative(freq)` inlined as `freq >= 0.0`, false for NaN).
fn scanCPUFrequencyFromCPUinfo(this: &mut LinuxMachine) {
    let existingCPUs = this.super_.existingCPUs;

    let content = match fs::read_to_string(PROCCPUINFOFILE) {
        Ok(c) => c,
        Err(_) => return,
    };

    let mut numCPUsWithFrequency = 0;
    let mut totalFrequency: f64 = 0.0;
    let mut cpuid: i32 = -1;

    for line in content.lines() {
        // Split "label : value" on the first ':'.
        let (label, value) = match line.split_once(':') {
            Some((l, v)) => (l.trim(), v.trim()),
            None => {
                if line.is_empty() {
                    cpuid = -1;
                }
                continue;
            }
        };

        if label == "processor" || label == "cpu number" {
            if let Ok(v) = value.parse::<i32>() {
                cpuid = v;
            }
            continue;
        }

        let frequency: Option<f64> = match label {
            "cpu MHz" | "CPU MHz" | "cpu MHz dynamic" => value.parse().ok(),
            "clock" => value.trim_end_matches("MHz").trim().parse().ok(),
            _ => None,
        };

        if let Some(frequency) = frequency {
            if cpuid < 0 || cpuid as u32 > existingCPUs - 1 {
                continue;
            }
            let cpuData = &mut this.cpuData[cpuid as usize + 1];
            /* do not override sysfs data */
            if !(cpuData.frequency >= 0.0) {
                cpuData.frequency = frequency;
            }
            numCPUsWithFrequency += 1;
            totalFrequency += frequency;
        } else if line.is_empty() {
            cpuid = -1;
        }
    }

    if numCPUsWithFrequency > 0 {
        this.cpuData[0].frequency = totalFrequency / numCPUsWithFrequency as f64;
    }
}

/// Port of `static void LinuxMachine_fetchCPUTopologyFromCPUinfo(
/// LinuxMachine* this)` from `LinuxMachine.c:651`. Reads `physical id` /
/// `core id` per CPU from `/proc/cpuinfo` (blank line ends each CPU block)
/// and records `maxPhysicalID` / `maxCoreID`.
fn LinuxMachine_fetchCPUTopologyFromCPUinfo(this: &mut LinuxMachine) {
    let existingCPUs = this.super_.existingCPUs;

    let content = match fs::read_to_string(PROCCPUINFOFILE) {
        Ok(c) => c,
        Err(_) => return,
    };

    let mut cpuid: i32 = -1;
    let mut coreid: i32 = -1;
    let mut physicalid: i32 = -1;

    let mut max_physicalid: i32 = -1;
    let mut max_coreid: i32 = -1;

    for line in content.lines() {
        if line.is_empty() {
            /* empty line after each cpu */
            if cpuid >= 0 && (cpuid as u32) < existingCPUs {
                let cpuData = &mut this.cpuData[cpuid as usize + 1];
                cpuData.coreID = coreid;
                cpuData.physicalID = physicalid;

                if coreid > max_coreid {
                    max_coreid = coreid;
                }
                if physicalid > max_physicalid {
                    max_physicalid = physicalid;
                }

                cpuid = -1;
                coreid = -1;
                physicalid = -1;
            }
        } else if String_startsWith(line, "processor") {
            if let Some(v) = line
                .split_once(':')
                .and_then(|(_, v)| v.trim().parse().ok())
            {
                cpuid = v;
            }
        } else if String_startsWith(line, "physical id") {
            if let Some(v) = line
                .split_once(':')
                .and_then(|(_, v)| v.trim().parse().ok())
            {
                physicalid = v;
            }
        } else if String_startsWith(line, "core id") {
            if let Some(v) = line
                .split_once(':')
                .and_then(|(_, v)| v.trim().parse().ok())
            {
                coreid = v;
            }
        }
    }

    this.maxPhysicalID = max_physicalid;
    this.maxCoreID = max_coreid;
}

/// Port of `static void LinuxMachine_assignCCDs(LinuxMachine* this, int
/// ccds)` from `LinuxMachine.c:702`. Distributes AMD CCD IDs across cores
/// (iterated by physical/core ID) assuming equal-size CCDs; `ccds == 0`
/// clears all `ccdID` to -1.
fn LinuxMachine_assignCCDs(this: &mut LinuxMachine, ccds: i32) {
    let existingCPUs = this.super_.existingCPUs;

    if ccds == 0 {
        for i in 0..existingCPUs as usize + 1 {
            this.cpuData[i].ccdID = -1;
        }
        return;
    }

    let coresPerCCD = existingCPUs as i32 / ccds;

    let mut ccd = 0;
    let mut nc = coresPerCCD;
    for p in 0..=this.maxPhysicalID {
        for c in 0..=this.maxCoreID {
            for i in 1..=existingCPUs as usize {
                if this.cpuData[i].physicalID != p || this.cpuData[i].coreID != c {
                    continue;
                }

                this.cpuData[i].ccdID = ccd;

                nc -= 1;
                if nc <= 0 {
                    nc = coresPerCCD;
                    ccd += 1;
                }
            }
        }
    }
}

/// Port of `static void LinuxMachine_computeThreadIndices(LinuxMachine*
/// this)` from `LinuxMachine.c:742`. Computes the SMT `threadIndex` and a
/// normalized `coreIndex` per CPU from shared physical/core IDs.
fn LinuxMachine_computeThreadIndices(this: &mut LinuxMachine) {
    let existingCPUs = this.super_.existingCPUs as usize;

    /* threadIndex: count lower-indexed CPUs sharing physicalID+coreID. */
    for i in 1..=existingCPUs {
        let mut threadIndex = 0;
        for j in 1..i {
            if this.cpuData[i].physicalID == this.cpuData[j].physicalID
                && this.cpuData[i].coreID == this.cpuData[j].coreID
            {
                threadIndex += 1;
            }
        }
        this.cpuData[i].threadIndex = threadIndex;
    }

    /* normalized physical core index. */
    let mut maxCoreIndex = 0;
    for i in 1..=existingCPUs {
        this.cpuData[i].coreIndex = maxCoreIndex;
        maxCoreIndex += 1;
        for j in (1..i).rev() {
            if this.cpuData[i].physicalID == this.cpuData[j].physicalID
                && this.cpuData[i].coreID == this.cpuData[j].coreID
            {
                debug_assert!(this.cpuData[i].threadIndex != this.cpuData[j].threadIndex);
                this.cpuData[i].coreIndex = this.cpuData[j].coreIndex;
                maxCoreIndex -= 1;
                break;
            }
        }
    }

    /* Set core & thread indices to zero for cpu0 (average) */
    this.cpuData[0].coreIndex = 0;
    this.cpuData[0].threadIndex = 0;
}

/// Port of `static void LinuxMachine_scanCPUFrequency(LinuxMachine* this)`
/// from `LinuxMachine.c:788`. Resets every CPU frequency to NaN, then
/// prefers the sysfs source and falls back to `/proc/cpuinfo`.
fn LinuxMachine_scanCPUFrequency(this: &mut LinuxMachine) {
    let existingCPUs = this.super_.existingCPUs;

    for i in 0..=existingCPUs as usize {
        this.cpuData[i].frequency = f64::NAN;
    }

    if scanCPUFrequencyFromSysCPUFreq(this) == 0 {
        return;
    }

    scanCPUFrequencyFromCPUinfo(this);
}

/// Port of `void Machine_scan(Machine* super)` from `LinuxMachine.c:800`.
/// Runs the per-scan memory / huge-page / ZFS / ZRAM / CPU-time passes,
/// then (when enabled) the CPU-frequency pass.
///
/// The `HAVE_SENSORS_SENSORS_H` `showCPUTemperature` clause and the
/// trailing `LibSensors_getCPUTemperatures` call are omitted (no-sensors
/// build variant; see module docs).
///
/// Port of `void Machine_scan(Machine* super)` from `LinuxMachine.c:786`.
/// Runs the per-scan `/proc` passes in C order, then — when
/// `settings->showCPUFrequency` is set — the CPU-frequency pass. C binds
/// `const Settings* settings = super->settings;` and dereferences it with no
/// null check, so this `expect`s `super->settings` present, matching C.
pub fn Machine_scan(this: &mut LinuxMachine) {
    LinuxMachine_scanMemoryInfo(this);
    LinuxMachine_scanHugePages(this);
    LinuxMachine_scanZfsArcstats(this);
    LinuxMachine_scanZramInfo(this);
    LinuxMachine_scanCPUTime(this);

    // C (LinuxMachine.c:809-815):
    //   const Settings* settings = super->settings;
    //   if (settings->showCPUFrequency /* || settings->showCPUTemperature */)
    //      LinuxMachine_scanCPUFrequency(this);
    // C dereferences `settings` unconditionally (never null here).
    let show_cpu_frequency = this
        .super_
        .settings
        .as_ref()
        .expect("Machine_scan: super->settings (C dereferences it unconditionally)")
        .showCPUFrequency;
    if show_cpu_frequency {
        LinuxMachine_scanCPUFrequency(this);
    }
}

/// TODO: port of `Machine* Machine_new(UsersTable* usersTable, uid_t
/// userId)` from `LinuxMachine.c:823`. Blocked: it calls `Machine_init`
/// (a documented stub in `machine.rs`, needs `getuid`/`Platform_*`/hwloc)
/// and depends on `sysconf(_SC_PAGESIZE)` / `sysconf(_SC_CLK_TCK)` plus
/// `LibSensors_countCCDs` (no-sensors variant → 0). The `/proc/stat`
/// btime read and the topology-init sequence
/// (`updateCPUcount`/`fetchCPUTopologyFromCPUinfo`/`assignCCDs`/
/// `computeThreadIndices`) are ready here, but the constructor cannot run
/// faithfully until `Machine_init` is ported.
pub fn Machine_new() {
    todo!("port of LinuxMachine.c:823 — blocked on Machine_init (machine.rs stub) + sysconf")
}

/// Deliberate non-port: `void Machine_delete(Machine* super)` from
/// `LinuxMachine.c:877` is a pure `free()` teardown — it walks and frees
/// the `gpuEngineData` linked list, `free`s `cpuData`, and calls
/// `Machine_done(super)` (itself a free/destroy teardown). Rust `Drop`
/// releases the owned `Vec`/`Box`/`String` fields of `LinuxMachine`, so
/// this has no ported body (rule 3).
pub fn Machine_delete() {
    todo!("deliberate non-port: free() teardown handled by Drop (LinuxMachine.c:877)")
}

/// Port of `bool Machine_isCPUonline(const Machine* super, unsigned int
/// id)` from `LinuxMachine.c:894`. The C `(const LinuxMachine*)super`
/// downcast is a `&LinuxMachine` here.
pub fn Machine_isCPUonline(this: &LinuxMachine, id: u32) -> bool {
    debug_assert!(id < this.super_.existingCPUs);
    this.cpuData[id as usize + 1].online
}

/// Port of `int Machine_getCPUPhysicalCoreID(const Machine* super,
/// unsigned int id)` from `LinuxMachine.c:901`. Returns the normalized
/// `coreIndex` of CPU `id`.
pub fn Machine_getCPUPhysicalCoreID(this: &LinuxMachine, id: u32) -> i32 {
    debug_assert!(id < this.super_.existingCPUs);
    this.cpuData[id as usize + 1].coreIndex
}

/// Port of `int Machine_getCPUThreadIndex(const Machine* super, unsigned
/// int id)` from `LinuxMachine.c:908`. Returns the SMT `threadIndex` of
/// CPU `id`.
pub fn Machine_getCPUThreadIndex(this: &LinuxMachine, id: u32) -> i32 {
    debug_assert!(id < this.super_.existingCPUs);
    this.cpuData[id as usize + 1].threadIndex
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a `LinuxMachine` with `n` physical CPUs (plus the aggregate
    /// at index 0), all online.
    fn machine_with_cpus(n: u32) -> LinuxMachine {
        let mut m = LinuxMachine {
            cpuData: vec![CPUData::default(); n as usize + 1],
            ..Default::default()
        };
        m.super_.existingCPUs = n;
        m.super_.activeCPUs = n;
        for c in m.cpuData.iter_mut() {
            c.online = true;
        }
        m
    }

    #[test]
    fn isCPUonline_reads_offset_by_one() {
        let mut m = machine_with_cpus(3);
        m.cpuData[2].online = false; // CPU id 1 => index 2
        assert!(Machine_isCPUonline(&m, 0));
        assert!(!Machine_isCPUonline(&m, 1));
        assert!(Machine_isCPUonline(&m, 2));
    }

    #[test]
    fn getCPU_indices_read_offset_by_one() {
        let mut m = machine_with_cpus(2);
        m.cpuData[1].coreIndex = 5;
        m.cpuData[1].threadIndex = 1;
        m.cpuData[2].coreIndex = 7;
        m.cpuData[2].threadIndex = 0;
        assert_eq!(Machine_getCPUPhysicalCoreID(&m, 0), 5);
        assert_eq!(Machine_getCPUThreadIndex(&m, 0), 1);
        assert_eq!(Machine_getCPUPhysicalCoreID(&m, 1), 7);
        assert_eq!(Machine_getCPUThreadIndex(&m, 1), 0);
    }

    #[test]
    fn assignCCDs_zero_clears_all_to_minus_one() {
        let mut m = machine_with_cpus(4);
        for c in m.cpuData.iter_mut() {
            c.ccdID = 42;
        }
        LinuxMachine_assignCCDs(&mut m, 0);
        for c in &m.cpuData {
            assert_eq!(c.ccdID, -1);
        }
    }

    #[test]
    fn assignCCDs_splits_cores_into_two_ccds() {
        // 4 cores, single socket, 2 CCDs => 2 cores per CCD.
        let mut m = machine_with_cpus(4);
        m.maxPhysicalID = 0;
        m.maxCoreID = 3;
        for (i, c) in m.cpuData.iter_mut().enumerate().skip(1) {
            c.physicalID = 0;
            c.coreID = i as i32 - 1;
        }
        LinuxMachine_assignCCDs(&mut m, 2);
        assert_eq!(m.cpuData[1].ccdID, 0);
        assert_eq!(m.cpuData[2].ccdID, 0);
        assert_eq!(m.cpuData[3].ccdID, 1);
        assert_eq!(m.cpuData[4].ccdID, 1);
    }

    #[test]
    fn computeThreadIndices_pairs_smt_siblings() {
        // 4 threads: cores {0,0,1,1} => two SMT pairs, thread indices 0/1,
        // coreIndex shared within each pair.
        let mut m = machine_with_cpus(4);
        for (i, c) in m.cpuData.iter_mut().enumerate().skip(1) {
            c.physicalID = 0;
            c.coreID = (i as i32 - 1) / 2; // 0,0,1,1
        }
        LinuxMachine_computeThreadIndices(&mut m);
        assert_eq!(m.cpuData[1].threadIndex, 0);
        assert_eq!(m.cpuData[2].threadIndex, 1);
        assert_eq!(m.cpuData[3].threadIndex, 0);
        assert_eq!(m.cpuData[4].threadIndex, 1);
        // siblings share a coreIndex; distinct cores differ.
        assert_eq!(m.cpuData[1].coreIndex, m.cpuData[2].coreIndex);
        assert_eq!(m.cpuData[3].coreIndex, m.cpuData[4].coreIndex);
        assert_ne!(m.cpuData[1].coreIndex, m.cpuData[3].coreIndex);
        // average reset to zero.
        assert_eq!(m.cpuData[0].coreIndex, 0);
        assert_eq!(m.cpuData[0].threadIndex, 0);
    }
}
