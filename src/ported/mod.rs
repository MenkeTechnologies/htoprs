//! Faithful ports of htop C source files.
//!
//! One Rust module per C file (module name = C file stem, lowercased).
//! Each `fn` here ports a specific htop C function and cites its
//! origin (`<File>.c:<line>`) in the doc comment. See `build.rs` for
//! the port-purity gate that enforces this.

pub mod action;
pub mod affinity;
pub mod affinitypanel;
pub mod availablecolumnspanel;
pub mod availablemeterspanel;
pub mod backtracescreen;
pub mod batterymeter;
pub mod categoriespanel;
pub mod colorspanel;
pub mod columnspanel;
pub mod commandline;
pub mod commandscreen;
pub mod cpumeter;
pub mod crt;
pub mod datetimemeter;
pub mod diskiometer;
pub mod displayoptionspanel;
pub mod dragonflybsd;
pub mod dynamiccolumn;
pub mod dynamicmeter;
pub mod dynamicscreen;
pub mod envscreen;
pub mod filedescriptormeter;
pub mod functionbar;
pub mod gpumeter;
pub mod hashtable;
pub mod header;
pub mod headeroptionspanel;
pub mod history;
pub mod hostnamemeter;
pub mod htop;
pub mod incset;
pub mod infoscreen;
pub mod lineeditor;
pub mod linux;
pub mod listitem;
pub mod loadaveragemeter;
pub mod machine;
pub mod mainpanel;
pub mod memorymeter;
pub mod memoryswapmeter;
pub mod meter;
pub mod meterspanel;
pub mod networkiometer;
pub mod object;
pub mod openfilesscreen;
pub mod optionitem;
pub mod panel;
pub mod process;
pub mod processlocksscreen;
pub mod processtable;
pub mod richstring;
pub mod row;
pub mod scheduling;
pub mod screenmanager;
pub mod screenspanel;
pub mod screentabspanel;
pub mod settings;
pub mod signalspanel;
pub mod swapmeter;
pub mod sysarchmeter;
pub mod table;
pub mod tasksmeter;
pub mod tracescreen;
pub mod uptimemeter;
pub mod userstable;
pub mod vector;
pub mod xutils;

// The Darwin platform layer binds mach / IOKit / darwin-only `sysctl`
// symbols, so it is compiled only on macOS — mirroring htop's per-platform
// build. The port-purity gate and port report scan the source as text, so
// coverage tracking is unaffected by the cfg.
#[cfg(target_os = "macos")]
pub mod darwin;

// The FreeBSD platform layer binds freebsd-only `sysctl` MIBs and structs
// (`ifmibdata`, `loadavg`, …), so it is compiled only on FreeBSD — mirroring
// htop's per-platform build. The port-purity gate and port report scan the
// source as text, so coverage tracking is unaffected by the cfg.
#[cfg(target_os = "freebsd")]
pub mod freebsd;

// The NetBSD platform layer binds netbsd-only `sysctl` MIBs / `getifaddrs`
// `if_data` and `loadavg`, so it is compiled only on NetBSD — mirroring htop's
// per-platform build. The port-purity gate and port report scan the source as
// text, so coverage tracking is unaffected by the cfg.
#[cfg(target_os = "netbsd")]
pub mod netbsd;

// The Solaris/illumos platform layer binds `libkstat`/`libproc`/`utmpx`
// symbols, so it is compiled only there — mirroring htop's per-platform
// build. The port-purity gate and port report scan the source as text, so
// coverage tracking is unaffected by the cfg.
#[cfg(any(target_os = "solaris", target_os = "illumos"))]
pub mod solaris;

// The OpenBSD platform layer binds openbsd-only `sysctl` MIBs and the
// `hw.sensors` battery structs, so it is compiled only on OpenBSD. NOTE:
// OpenBSD is a tier-3 Rust target with no prebuilt std, so this module is
// verified by the port-purity gate + primary-source libc reading, not a
// cross-compile. The gate/report scan source as text, unaffected by the cfg.
#[cfg(target_os = "openbsd")]
pub mod openbsd;
pub mod unsupported;
