//! Port of htop's `generic/` — the platform-independent helpers each
//! platform's `Platform.c` aliases via `#define` (e.g. the per-platform
//! `#define Platform_gettime_realtime Generic_gettime_realtime`). Compiled on
//! every target so the shared `Machine`/meter code can call them regardless of
//! which platform module is active.
#![allow(non_snake_case)]

pub mod fdstat_sysctl;
pub mod gettime;
