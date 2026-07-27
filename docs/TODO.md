That string is at src/ported/commandline.rs:773, inside a deliberate non-darwin stub:

  src/ported/commandline.rs:572  #[cfg(target_os = "macos")]        pub fn CommandLine_run  ← full assembly
  src/ported/commandline.rs:766  #[cfg(not(target_os = "macos"))]   pub fn CommandLine_run  ← parses argv, prints, returns 1

  So it isn't a runtime detection failure — the Linux runtime object graph was never assembled. cargo check --locked --target x86_64-unknown-linux-gnu
  --bin htoprs → Finished dev profile ... in 10.24s, so the crate builds for Linux; only the entry path is gated.

  What already exists on the Linux side:

  ┌──────────────────────────────────────────────────────────┬──────────────────────────────────────────────────────────────────────────────────────┐
  │                          piece                           │                                       location                                       │
  ├──────────────────────────────────────────────────────────┼──────────────────────────────────────────────────────────────────────────────────────┤
  │ Machine_new / Machine_scan                               │ src/ported/linux/linuxmachine.rs:1103, :1059                                         │
  ├──────────────────────────────────────────────────────────┼──────────────────────────────────────────────────────────────────────────────────────┤
  │ ProcessTable_new / goThroughEntries                      │ src/ported/linux/linuxprocesstable.rs:431, :2912                                     │
  ├──────────────────────────────────────────────────────────┼──────────────────────────────────────────────────────────────────────────────────────┤
  │ Platform_init, Platform_meterTypes,                      │ src/ported/linux/platform.rs:1841, :111, :143                                        │
  │ Platform_memoryClasses                                   │                                                                                      │
  ├──────────────────────────────────────────────────────────┼──────────────────────────────────────────────────────────────────────────────────────┤
  │ meter dispatch                                           │ already #[cfg(not(target_os = "macos"))] use linux::platform::… (e.g.                │
  │                                                          │ swapmeter.rs:42)                                                                     │
  └──────────────────────────────────────────────────────────┴──────────────────────────────────────────────────────────────────────────────────────┘

  Three real holes beyond the stub, all verified:

  1. No LinuxProcessTable_class. Only DarwinProcessTable_class exists (darwinprocesstable.rs:311), and darwin sets super_.super_.klass at :344. Linux
  ProcessTable_new (:431-461) never sets klass, so Table_scanPrepare/iterate/cleanup fall back to the base defaults and ProcessTable_goThroughEntries
  never fires — empty process list.
  2. Realtime resample is macOS-only. screenmanager.rs:366-375 calls darwin::platform::Platform_gettime_realtime with no not(macos) arm, while
  generic/gettime.rs:13 Generic_gettime_realtime (what htop's Linux macro aliases) is ported and unused there. host.realtimeMs would stay 0, so the
  delay gate at :380-389 never advances.
  3. Constructor shape mismatch. Darwin returns Box<DarwinProcessTable> (:331); Linux returns the value (:431). machine.rs:293-294 also has no Linux
  free branch (leak-only, matches C's exit-time behavior).

  Also note release.yml:33,35 ships x86_64/aarch64 Linux tarballs whose binaries hit this stub, and README.md:141 only says "daily driver on macOS"
  without stating the Linux artifact is non-interactive.
