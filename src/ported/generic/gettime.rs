//! Port of `generic/gettime.c` — the clock samplers the per-platform
//! `Platform_gettime_*` macros alias to.
#![allow(non_snake_case)]

use std::mem::zeroed;

/// Port of `void Generic_gettime_realtime(struct timeval* tvp, uint64_t* msec)`
/// from `generic/gettime.c:16`. Samples `CLOCK_REALTIME` into a `timeval` plus
/// a millisecond stamp; on failure zeroes both. This is the
/// `HAVE_CLOCK_GETTIME` branch — `clock_gettime` is available on every
/// supported target, so the `gettimeofday` fallback is not built (matching a
/// `HAVE_CLOCK_GETTIME` configuration).
pub fn Generic_gettime_realtime(tvp: &mut libc::timeval, msec: &mut u64) {
    let mut ts: libc::timespec = unsafe { zeroed() };
    if unsafe { libc::clock_gettime(libc::CLOCK_REALTIME, &mut ts) } == 0 {
        tvp.tv_sec = ts.tv_sec;
        tvp.tv_usec = (ts.tv_nsec / 1000) as libc::suseconds_t;
        *msec = (ts.tv_sec as u64 * 1000) + (ts.tv_nsec as u64 / 1_000_000);
    } else {
        *tvp = unsafe { zeroed() };
        *msec = 0;
    }
}

/// Port of `void Generic_gettime_monotonic(uint64_t* msec)` from
/// `generic/gettime.c:44`. Samples `CLOCK_MONOTONIC` into a millisecond stamp;
/// on failure zeroes it. `HAVE_CLOCK_GETTIME` branch, as above.
pub fn Generic_gettime_monotonic(msec: &mut u64) {
    let mut ts: libc::timespec = unsafe { zeroed() };
    if unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) } == 0 {
        *msec = (ts.tv_sec as u64 * 1000) + (ts.tv_nsec as u64 / 1_000_000);
    } else {
        *msec = 0;
    }
}
