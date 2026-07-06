//! Port of htop's `generic/` — the platform-independent helpers each
//! platform's `Platform.c` aliases via `#define` (e.g. the per-platform
//! `#define Platform_gettime_realtime Generic_gettime_realtime`). Compiled on
//! every target so the shared `Machine`/meter code can call them regardless of
//! which platform module is active.
#![allow(non_snake_case)]

// htop's `HAVE_DEMANGLING` build variant (libiberty `cplus_demangle`), behind
// the `demangle` cargo feature — off by default, verified by reading + the gate.
#[cfg(feature = "demangle")]
pub mod demangle;
pub mod fdstat_sysctl;
pub mod gettime;
// OpenZFS ARC stats via `kstat.zfs.misc.arcstats.*` sysctls — used by the FreeBSD
// and Darwin machine backends. Gated to the `sysctl`-having platforms (Linux
// reads ARC from `/proc`); real-compiled on darwin.
#[cfg(any(
    target_os = "macos",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly"
))]
pub mod openzfs_sysctl;
pub mod uname;
// htop's `HAVE_LIBUNWIND_PTRACE` build variant (libunwind-ptrace backtrace
// backend), behind the `unwind` cargo feature — off by default, verified by
// reading the libunwind headers + the gate (libunwind does not link on macOS).
#[cfg(feature = "unwind")]
pub mod unwindptrace;
