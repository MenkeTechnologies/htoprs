//! htoprs-original extensions.
//!
//! Unlike [`crate::ported`], code here is not a 1:1 translation of htop's
//! C source and is therefore exempt from the `build.rs` port-purity gate.
//! It carries htoprs-only capabilities layered on top of the faithful port.
//!
//! [`theme`] holds the named color-scheme system ported from iftoprs
//! (originally from storageshower): 31 built-in 6-color palettes plus the
//! custom-theme plumbing. [`overlay`] holds the themed keyboard-help overlay,
//! theme chooser, and theme editor (also ported from iftoprs), which render
//! into a `ratatui::Buffer` using those palettes, plus the live-wiring
//! (`dispatch_key` / `draw_active`) the running TUI drives them through.
//! [`colors`] makes a selected theme recolor the actual htop UI in 256-color
//! via a base16-style ANSI palette remap consulted by `Ncurses::to_color`.
//!
//! The remaining modules are htoprs-original monitoring capabilities htop
//! lacks, built against [`model::Proc`]:
//! [`procring`] per-process CPU/mem history + sparkline column,
//! [`finder`] fuzzy process finder, [`snapshot`] capture + diff a table,
//! [`filter`] regex + saved named filters, [`export`] table -> JSON/CSV,
//! [`alerts`] debounced threshold rules, [`graph`] braille history graph.
//! [`braille`] is the shared glyph renderer used by [`procring`] and [`graph`].
//! [`bridge`] materializes the live ported `Process` rows as `Proc`, and
//! [`panels`] is the running-TUI wiring — a thread-local state the run loop
//! feeds each refresh, dispatches keys through, and draws over the panels
//! (the monitoring analog of [`overlay`]).
//! [`help`] renders the styled `htoprs -h` screen (figlet banner + status box
//! + sectioned option list) shown in place of the plain ported `printHelpFlag`.
//! [`barstyle`] is the `b`-key bar fill-glyph cycler ported from storageshower
//! (Classic → Gradient → Solid → Thin → Ascii), consulted by the ported
//! `BarMeterMode_draw` fill loop and wired into the keybinding table as an
//! [`Htop_Action`].
//!
//! [`Htop_Action`]: crate::ported::action::Htop_Action

pub mod colors;
pub mod overlay;
pub mod prefs;
pub mod theme;

pub mod alerts;
pub mod barstyle;
pub mod braille;
pub mod bridge;
pub mod export;
pub mod filter;
pub mod frame;
pub mod finder;
pub mod graph;
pub mod help;
pub mod model;
pub mod panels;
pub mod procring;
pub mod snapshot;
