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
pub mod uname;
// htop's `HAVE_LIBUNWIND_PTRACE` build variant (libunwind-ptrace backtrace
// backend), behind the `unwind` cargo feature — off by default, verified by
// reading the libunwind headers + the gate (libunwind does not link on macOS).
#[cfg(feature = "unwind")]
pub mod unwindptrace;
