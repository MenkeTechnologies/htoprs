//! Stub scaffold for `DateTimeMeter.c` — NOT yet ported.
//!
//! Every `pub fn` below is a placeholder (`todo!()`) named after a real
//! htop C function so the port-purity gate accepts the module and the
//! port surface is laid out. Replace each stub with a faithful port of
//! the C body, updating the signature and the doc comment to `Port of
//! `DateTimeMeter.c`:<line>.` as you go. `gen_port_report.py` counts these
//! `todo!()` bodies as *stubbed*, not *ported*, so scaffolding does not
//! inflate coverage.
#![allow(non_snake_case)]
#![allow(dead_code)]

/// TODO: port of `static void DateTimeMeter_updateValues(Meter* this)` from
/// `DateTimeMeter.c:32`. Blocked on two missing dependencies:
///
/// 1. `host->realtime.tv_sec` — the C reads the current sample time as a
///    `struct timeval` off the `Machine` host, but the ported
///    [`Machine`](crate::ported::machine::Machine) models only `realtimeMs`
///    (a `u64`), not the C `struct timeval realtime` field, so `tv_sec` is
///    unreachable (the same blocker keeps `Process_fillStarttimeBuffer` and
///    `GraphMeterMode_draw` stubbed).
/// 2. The three-way `As_Meter(this)` class dispatch — this single C function
///    backs `ClockMeter_class`, `DateMeter_class`, and `DateTimeMeter_class`
///    and branches on which concrete class the meter is (`As_Meter(this) ==
///    &ClockMeter_class`, etc.). The ported [`Meter`](crate::ported::meter::Meter)
///    carries no per-instance concrete `MeterClass`: its `klass()` always
///    returns `&Meter_class.super_`, so the concrete class cannot be
///    recovered to pick the `"%H:%M:%S"` / `"%F"` / `"%F %H:%M:%S"` branch.
///
/// The remaining work (`localtime_r` + `strftime`) is pure and doable via
/// `libc` once the two inputs above are reachable.
pub fn DateTimeMeter_updateValues() {
    todo!(
        "port of DateTimeMeter.c:32 — needs host->realtime.tv_sec (Machine models only realtimeMs) + per-instance concrete MeterClass for As_Meter dispatch"
    )
}
