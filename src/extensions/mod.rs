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
//! into a `ratatui::Buffer` using those palettes.

pub mod overlay;
pub mod theme;
