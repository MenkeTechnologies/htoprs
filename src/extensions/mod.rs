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

pub mod colors;
pub mod overlay;
pub mod theme;
