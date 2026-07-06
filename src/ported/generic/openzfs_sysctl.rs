//! Port of `generic/openzfs_sysctl.c` — the OpenZFS ARC statistics reader used
//! by the FreeBSD and Darwin machine backends (`Machine_scan` /`Machine_new`).
//!
//! Pure `sysctl` (`kstat.zfs.misc.arcstats.*`), no external ZFS library — so it
//! compiles and runs on the darwin target (verified there, not tier-3). Gated to
//! the `sysctl`-having platforms; Linux reads ARC stats from `/proc` instead
//! (`LinuxMachine`), so it does not use this module.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]

use std::os::raw::c_int;
use std::ptr;
use std::sync::Mutex;

use crate::ported::linux::linuxmachine::ZfsArcStats;

/// The cached MIBs the C keeps as ten file-scope `static int MIB_...[5]` arrays,
/// resolved once by [`openzfs_sysctl_init`] and reused by
/// [`openzfs_sysctl_updateArcStats`]. Grouped behind one process-global `Mutex`
/// (the faithful analog of the C's shared file-scope state).
struct ZfsArcMibs {
    size: [c_int; 5],
    c_min: [c_int; 5],
    c_max: [c_int; 5],
    mfu_size: [c_int; 5],
    mru_size: [c_int; 5],
    anon_size: [c_int; 5],
    hdr_size: [c_int; 5],
    other_size: [c_int; 5],
    compressed_size: [c_int; 5],
    uncompressed_size: [c_int; 5],
}

static ZFS_MIBS: Mutex<ZfsArcMibs> = Mutex::new(ZfsArcMibs {
    size: [0; 5],
    c_min: [0; 5],
    c_max: [0; 5],
    mfu_size: [0; 5],
    mru_size: [0; 5],
    anon_size: [0; 5],
    hdr_size: [0; 5],
    other_size: [0; 5],
    compressed_size: [0; 5],
    uncompressed_size: [0; 5],
});

/// Port of `void openzfs_sysctl_init(ZfsArcStats* stats)`
/// (`generic/openzfs_sysctl.c:28`). Probes `kstat.zfs.misc.arcstats.size`; if ZFS
/// is present (present and non-zero), marks the stats enabled, caches every
/// arcstats MIB, and records whether the compressed-size counters exist.
pub fn openzfs_sysctl_init(stats: &mut ZfsArcStats) {
    // C: `len = 5; sysctlnametomib(name, mib, &len)` — resolve one arcstats name
    // into its cached MIB (rc 0 = found, used to set `isCompressed`). Nested to
    // stay a faithful translation without a module-level non-C helper fn.
    fn nametomib(name: &[u8], mib: &mut [c_int; 5]) -> c_int {
        let mut len: libc::size_t = 5;
        unsafe { libc::sysctlnametomib(name.as_ptr() as *const _, mib.as_mut_ptr(), &mut len) }
    }

    // if (sysctlbyname("kstat.zfs.misc.arcstats.size", &arcSize, &len, NULL, 0) == 0 && arcSize != 0)
    let mut arc_size: libc::c_ulonglong = 0;
    let mut len: libc::size_t = std::mem::size_of::<libc::c_ulonglong>();
    let ok = unsafe {
        libc::sysctlbyname(
            c"kstat.zfs.misc.arcstats.size".as_ptr(),
            &mut arc_size as *mut _ as *mut libc::c_void,
            &mut len,
            ptr::null_mut(),
            0,
        )
    };
    if ok == 0 && arc_size != 0 {
        stats.enabled = 1;

        let mut mibs = ZFS_MIBS.lock().unwrap();
        nametomib(b"kstat.zfs.misc.arcstats.size\0", &mut mibs.size);
        nametomib(b"kstat.zfs.misc.arcstats.c_min\0", &mut mibs.c_min);
        nametomib(b"kstat.zfs.misc.arcstats.c_max\0", &mut mibs.c_max);
        nametomib(b"kstat.zfs.misc.arcstats.mfu_size\0", &mut mibs.mfu_size);
        nametomib(b"kstat.zfs.misc.arcstats.mru_size\0", &mut mibs.mru_size);
        nametomib(b"kstat.zfs.misc.arcstats.anon_size\0", &mut mibs.anon_size);
        nametomib(b"kstat.zfs.misc.arcstats.hdr_size\0", &mut mibs.hdr_size);
        nametomib(
            b"kstat.zfs.misc.arcstats.other_size\0",
            &mut mibs.other_size,
        );

        // isCompressed iff the compressed_size MIB resolves.
        if nametomib(
            b"kstat.zfs.misc.arcstats.compressed_size\0",
            &mut mibs.compressed_size,
        ) == 0
        {
            stats.isCompressed = 1;
            nametomib(
                b"kstat.zfs.misc.arcstats.uncompressed_size\0",
                &mut mibs.uncompressed_size,
            );
        } else {
            stats.isCompressed = 0;
        }
    } else {
        stats.enabled = 0;
    }
}

/// Port of `void openzfs_sysctl_updateArcStats(ZfsArcStats* stats)`
/// (`generic/openzfs_sysctl.c:58`). When ZFS is enabled, refreshes every ARC
/// counter from its cached MIB (converting bytes → KiB), including the
/// compressed/uncompressed sizes when present.
pub fn openzfs_sysctl_updateArcStats(stats: &mut ZfsArcStats) {
    // C: `len = sizeof(x); sysctl(mib, 5, &x, &len, NULL, 0); x /= 1024` — read
    // one cached-MIB arcstats counter into `out`, bytes → KiB. Nested to avoid a
    // module-level non-C helper fn.
    fn read_kib(mib: &[c_int; 5], out: &mut u64) {
        let mut len: libc::size_t = std::mem::size_of::<u64>();
        unsafe {
            libc::sysctl(
                mib.as_ptr() as *mut c_int,
                5,
                out as *mut u64 as *mut libc::c_void,
                &mut len,
                ptr::null_mut(),
                0,
            );
        }
        *out /= 1024;
    }

    if stats.enabled == 0 {
        return;
    }
    let mibs = ZFS_MIBS.lock().unwrap();
    read_kib(&mibs.size, &mut stats.size);
    read_kib(&mibs.c_min, &mut stats.min);
    read_kib(&mibs.c_max, &mut stats.max);
    read_kib(&mibs.mfu_size, &mut stats.MFU);
    read_kib(&mibs.mru_size, &mut stats.MRU);
    read_kib(&mibs.anon_size, &mut stats.anon);
    read_kib(&mibs.hdr_size, &mut stats.header);
    read_kib(&mibs.other_size, &mut stats.other);

    if stats.isCompressed != 0 {
        read_kib(&mibs.compressed_size, &mut stats.compressed);
        read_kib(&mibs.uncompressed_size, &mut stats.uncompressed);
    }
}
