# TODO

## Linux TUI gate — fixed, pending runtime verification

`htoprs` on Linux used to print

```
htoprs: the interactive TUI is wired for macOS (darwin) in this build
```

and exit 1. The cause was not runtime detection: the Linux runtime object graph
was never assembled — `CommandLine_run` had a `#[cfg(target_os = "macos")]` full
body and a `#[cfg(not(target_os = "macos"))]` stub that only parsed argv.

Wired now:

| piece | location |
|---|---|
| `LinuxProcessTable_class` scan vtable + `klass` wiring (C `LinuxProcessTable.c:263` `Object_setClass(this, Class(ProcessTable))`) | `src/ported/linux/linuxprocesstable.rs` |
| `Platform_gettime_realtime` (C `linux/Platform.h:120` → `Generic_gettime_realtime`) | `src/ported/linux/platform.rs` |
| realtime resample arm in `checkRecalculation` | `src/ported/screenmanager.rs` |
| `Platform_signals` / `Platform_numberOfSignals` (C `linux/Platform.c:104`) | `src/ported/linux/platform.rs` |
| `CommandLine_run` / `setCommFilter` generalized to `any(macos, linux)`; `Platform_init` failure now returns 1 as in C | `src/ported/commandline.rs` |
| Linux process-table free branch in `Machine_delete` | `src/ported/machine.rs` |

Verified: `cargo clippy --all-targets -D warnings` and a full `cargo build`
link (via `zig cc -target x86_64-linux-gnu`) for `x86_64-unknown-linux-gnu`,
plus the native macOS suite. **Not yet verified: running the resulting binary on
a real Linux host** — no container runtime was available on the build machine.

## Remaining Linux gaps

- `Platform_meterTypes` (`src/ported/linux/platform.rs`) lists 27 of the C
  table's 49 entries (`linux/Platform.c`). Missing: `MemorySwapMeter`,
  `HugePageMeter`, the six `PressureStall*`, `ZfsArcMeter`,
  `ZfsCompressedArcMeter`, `ZramMeter`, the three `DiskIO*`, `NetworkIOMeter`,
  `SELinuxMeter`, `SystemdMeter`, `SystemdUserMeter`, `OpenRCMeter`,
  `OpenRCUserMeter`, `FileDescriptorMeter`, `GPUMeter`. Each is blocked on its
  own `MeterClass` static — no `pub static *_class` exists in
  `src/ported/linux/{pressurestallmeter,zrammeter,hugepagemeter,selinuxmeter,systemdmeter,openrcmeter}.rs`
  — so they do not appear in Setup's available-meters list on Linux.
- `release.yml` ships x86_64/aarch64 Linux tarballs; re-run the release smoke
  test on those artifacts once the runtime check above passes.
