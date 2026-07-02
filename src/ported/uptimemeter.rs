//! Partial port of `UptimeMeter.c` — htop's uptime meters.
//!
//! Both `updateValues` bodies are blocked on the same dependency: their
//! first statement is `int totalseconds = Platform_getUptime();`, and the
//! ported [`Platform_getUptime`](crate::ported::linux::platform::Platform_getUptime)
//! is still a `todo!()` stub whose signature returns `()` (unit), not the
//! `int` the C consumes. There is no faithful integer uptime source to feed
//! the seconds/minutes/hours/days arithmetic, so both functions stay
//! stubbed until the platform uptime reader lands. Every other input each
//! body touches — `this->txtBuffer` (modeled by
//! [`crate::ported::meter::Meter`]) and the pure `xSnprintf` formatting — is
//! already reachable; only the uptime value is missing.
#![allow(non_snake_case)]
#![allow(dead_code)]

/// TODO: port of `static void UptimeMeter_updateValues(Meter* this)` from
/// `UptimeMeter.c:22`. Blocked: the body opens with `int totalseconds =
/// Platform_getUptime();`, but the ported `Platform_getUptime`
/// (`linux/platform.rs`, still a `todo!()`) returns `()` rather than the
/// `int` this function needs. Without a real integer uptime the
/// `totalseconds <= 0` guard and the `% 60` / `/ 3600` / `/ 86400`
/// seconds→days breakdown that fills `this->txtBuffer` have no source to
/// compute from. The rest (the `daysbuf` `"%d days"` prefix and the
/// `"%s%02d:%02d:%02d"` format into `txtBuffer`) is pure and doable once
/// `Platform_getUptime` returns an `int`.
pub fn UptimeMeter_updateValues() {
    todo!("port of UptimeMeter.c:22: needs Platform_getUptime to return int (linux/platform.rs stub returns ())")
}

/// TODO: port of `static void SecondsUptimeMeter_updateValues(Meter* this)`
/// from `UptimeMeter.c:64`. Blocked for the same reason as
/// [`UptimeMeter_updateValues`]: the body is `int totalseconds =
/// Platform_getUptime(); ... xSnprintf(this->txtBuffer, ..., "%d s",
/// totalseconds)`, and the ported `Platform_getUptime` is still a `todo!()`
/// stub returning `()` instead of `int`, so there is no uptime value to
/// format into `this->txtBuffer`.
pub fn SecondsUptimeMeter_updateValues() {
    todo!("port of UptimeMeter.c:64: needs Platform_getUptime to return int (linux/platform.rs stub returns ())")
}
