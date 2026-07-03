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
pub mod dragonflybsd;
pub mod displayoptionspanel;
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
