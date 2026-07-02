//! Port of `SignalsPanel.c` — htop's "Send signal:" selection panel.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module — matching the spec
//! name-for-name is the point of the port.
//!
//! # Ported
//!
//! - [`SignalItem`] — the `struct SignalItem_ { const char* name; int
//!   number; }` from `SignalsPanel.h:17`. The header owns this type, so it
//!   is modeled here. `const char*` becomes `&'static str` (the platform
//!   tables are static string literals); `int number` becomes `c_int`.
//! - [`SIGNALSPANEL_INITSELECTEDSIGNAL`] — the `#define ... SIGTERM`
//!   (`SignalsPanel.h:22`), the default pre-selected signal.
//! - [`SignalsPanel_new`] (`SignalsPanel.c:23`) — builds the panel: a
//!   1×1 [`Panel`] with an Enter/Esc [`FunctionBar`] ("Send   "/"Cancel "),
//!   one [`ListItem`] per signal (name + number), a `defaultPosition`
//!   tracking the pre-selected signal, the optional Linux real-time-signal
//!   rows, the "Send signal:" header, and the initial selection.
//!
//! # Adaptation of the platform signal table
//!
//! The C body reads two globals — `Platform_signals[]` and
//! `Platform_numberOfSignals` — that are defined per-platform in
//! `linux/Platform.c` / `darwin/Platform.c`, neither of which is ported
//! yet (there is no `platform` module in the ported tree). Rather than
//! stub the whole function on that missing data, the table is *injected*
//! as a `signals: &[SignalItem]` parameter — the same adaptation
//! `History_new` uses for its `filename` input. Every algorithmic step of
//! the C body (the build loop, `defaultPosition` tracking, the RT-signal
//! rows, the header, the clamped selection) is ported faithfully; only the
//! source of the signal rows moves from an unported global to a parameter.
//! The caller supplies `Platform_signals` (a `&Platform_numberOfSignals`-
//! length slice) once that platform module lands.
//!
//! The C `Panel_set(this, i, ...)` writes sequential indices `0, 1, 2, …`
//! into a fresh (empty) panel; htop's `Vector_set` grows the vector when
//! `i == size`, so that is an append. The ported [`Panel_set`] indexes and
//! panics out of range, so the faithful observable equivalent here is
//! [`Panel_add`] (append), which reproduces the same resulting item order.
//!
//! # Stubbed
//!
//! None — `SignalsPanel.c` declares only `SignalsPanel_new`, which is
//! ported above.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use core::ffi::c_int;

use crate::ported::functionbar::FunctionBar_newEnterEsc;
use crate::ported::listitem::ListItem_new;
use crate::ported::object::Object;
use crate::ported::panel::{Panel, Panel_add, Panel_new, Panel_setHeader, Panel_setSelected};

/// Port of `typedef struct SignalItem_ { const char* name; int number; }
/// SignalItem` from `SignalsPanel.h:17`. `name` is the display label
/// (`const char*`, a static literal in the platform tables) and `number`
/// is the signal number passed to `kill(2)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SignalItem {
    pub name: &'static str,
    pub number: c_int,
}

/// Port of `#define SIGNALSPANEL_INITSELECTEDSIGNAL SIGTERM` from
/// `SignalsPanel.h:22`. `SIGTERM` is 15 on both Linux and Darwin (the two
/// ported targets); `libc::SIGTERM` is that constant.
pub const SIGNALSPANEL_INITSELECTEDSIGNAL: c_int = libc::SIGTERM;

/// Port of `Panel* SignalsPanel_new(int preSelectedSignal)` from
/// `SignalsPanel.c:23`.
///
/// Builds a `Panel_new(1, 1, 1, 1, …, FunctionBar_newEnterEsc("Send   ",
/// "Cancel "))`, then for each `SignalItem` appends a `ListItem_new(name,
/// number)` (see the module docs on `Panel_set` → [`Panel_add`]), tracking
/// `defaultPosition` — which starts at the literal `15` and is updated to
/// the index of the signal whose number equals `preSelectedSignal` (the C
/// comment notes signal 15 is not always the 15th table row). On Linux,
/// when `SIGRTMAX - SIGRTMIN <= 100`, one row per real-time signal is
/// appended (`"%2d SIGRTMIN%-+3d"`, truncated to `"…SIGRTMIN"` for `n ==
/// 0`); Darwin defines neither `SIGRTMIN` nor `SIGRTMAX`, so that `#if`
/// block is compiled out there. Finishes with `Panel_setHeader("Send
/// signal:")` and `Panel_setSelected(defaultPosition)` (which clamps).
///
/// `signals` supplies the C `Platform_signals[]` / `Platform_numberOfSignals`
/// pair — see the module docs for why it is a parameter.
pub fn SignalsPanel_new(preSelectedSignal: c_int, signals: &[SignalItem]) -> Panel {
    let mut this = Panel_new(
        1,
        1,
        1,
        1,
        Some(FunctionBar_newEnterEsc("Send   ", "Cancel ")),
    );
    let mut defaultPosition: c_int = 15;
    for (i, sig) in signals.iter().enumerate() {
        let item: Box<dyn Object> = Box::new(ListItem_new(sig.name, sig.number));
        Panel_add(&mut this, item);
        // signal 15 is not always the 15th signal in the table
        if sig.number == preSelectedSignal {
            defaultPosition = i as c_int;
        }
    }
    // C: #if (defined(SIGRTMIN) && defined(SIGRTMAX)) — Linux real-time
    // signals. Darwin defines neither macro, so the block does not exist
    // there; `cfg(target_os = "linux")` reproduces the `#if`.
    #[cfg(target_os = "linux")]
    {
        let sigrtmin = libc::SIGRTMIN();
        let sigrtmax = libc::SIGRTMAX();
        if sigrtmax - sigrtmin <= 100 {
            let mut sig = sigrtmin;
            while sig <= sigrtmax {
                let n = sig - sigrtmin;
                // xSnprintf(buf, 16, "%2d SIGRTMIN%-+3d", sig, n)
                let mut buf = format!("{:2} SIGRTMIN{:<+3}", sig, n);
                if n == 0 {
                    // buf[11] = '\0' — keep "%2d SIGRTMIN" (11 bytes for a
                    // 2-digit sig), dropping the "%-+3d" suffix.
                    buf.truncate(11);
                }
                let item: Box<dyn Object> = Box::new(ListItem_new(&buf, sig));
                Panel_add(&mut this, item);
                sig += 1;
            }
        }
    }
    Panel_setHeader(&mut this, "Send signal:");
    Panel_setSelected(&mut this, defaultPosition);
    this
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ported::listitem::ListItem;
    use crate::ported::panel::{Panel_get, Panel_size};

    /// A 16-row table where row `i` carries signal number `i`, so an item's
    /// list index equals its signal number — keeps assertions unambiguous.
    fn indexed_signals() -> Vec<SignalItem> {
        [
            " 0 Cancel", " 1 SIGHUP", " 2 SIGINT", " 3 SIGQUIT", " 4 SIGILL", " 5 SIGTRAP",
            " 6 SIGABRT", " 7 SIGEMT", " 8 SIGFPE", " 9 SIGKILL", "10 SIGBUS", "11 SIGSEGV",
            "12 SIGSYS", "13 SIGPIPE", "14 SIGALRM", "15 SIGTERM",
        ]
        .iter()
        .enumerate()
        .map(|(i, name)| SignalItem { name, number: i as c_int })
        .collect()
    }

    /// The `value` string of the panel's `i`-th `ListItem`.
    fn item_value(p: &Panel, i: c_int) -> String {
        let any: &dyn std::any::Any = Panel_get(p, i);
        any.downcast_ref::<ListItem>().unwrap().value.clone()
    }

    #[test]
    fn builds_one_item_per_signal_in_order() {
        let sigs = indexed_signals();
        let p = SignalsPanel_new(SIGNALSPANEL_INITSELECTEDSIGNAL, &sigs);
        // At least one item per injected signal (Linux may append RT rows).
        assert!(Panel_size(&p) >= sigs.len() as c_int);
        for (i, sig) in sigs.iter().enumerate() {
            assert_eq!(item_value(&p, i as c_int), sig.name);
        }
    }

    #[test]
    fn header_is_send_signal() {
        use crate::ported::richstring::RichString_sizeVal;
        let p = SignalsPanel_new(15, &indexed_signals());
        assert_eq!(RichString_sizeVal(&p.header), "Send signal:".len() as c_int);
    }

    #[test]
    fn function_bar_is_send_cancel() {
        let p = SignalsPanel_new(15, &indexed_signals());
        let bar = p.currentBar.as_ref().expect("currentBar set");
        assert_eq!(bar.functions, vec!["Send   ".to_string(), "Cancel ".to_string()]);
        assert_eq!(bar.keys, vec!["Enter".to_string(), "Esc".to_string()]);
        assert_eq!(bar.events, vec![13, 27]);
    }

    #[test]
    fn preselected_signal_sets_selection_to_its_index() {
        let sigs = indexed_signals();
        // Row index == signal number in this table, so number 9 is index 9.
        let p = SignalsPanel_new(9, &sigs);
        assert_eq!(p.selected, 9);
        assert_eq!(item_value(&p, p.selected), " 9 SIGKILL");
    }

    #[test]
    fn absent_preselected_signal_keeps_default_15_clamped() {
        // No row has number 999, so defaultPosition stays at the literal 15;
        // Panel_setSelected clamps it into [0, size-1].
        let sigs = indexed_signals();
        let p = SignalsPanel_new(999, &sigs);
        let expected = 15.min(Panel_size(&p) - 1);
        assert_eq!(p.selected, expected);
    }

    #[test]
    fn selection_is_clamped_when_default_exceeds_size() {
        // A 3-row table (numbers 0..2), no match for pre-select -> default
        // 15 is clamped down to the last valid index (size - 1). On Linux
        // the RT rows would raise the size, so compare against the clamp.
        let sigs: Vec<SignalItem> = (0..3)
            .map(|i| SignalItem { name: " x", number: i })
            .collect();
        let p = SignalsPanel_new(-1, &sigs);
        assert_eq!(p.selected, 15.min(Panel_size(&p) - 1));
        assert!(p.selected >= 0);
    }

    #[test]
    fn empty_table_still_sets_header_and_bar() {
        // Degenerate input: no injected signals. On Darwin the panel is
        // empty; Panel_setSelected(15) clamps to 0. (On Linux RT rows fill
        // it.) The header/bar must still be configured.
        use crate::ported::richstring::RichString_sizeVal;
        let p = SignalsPanel_new(15, &[]);
        assert_eq!(RichString_sizeVal(&p.header), "Send signal:".len() as c_int);
        assert!(p.selected >= 0);
    }

    #[test]
    fn rt_signal_label_format_matches_c_snprintf() {
        // White-box check of the "%2d SIGRTMIN%-+3d" formatting used by the
        // Linux RT-signal branch (which is cfg'd out on non-Linux hosts).
        // n == 0: truncate to "%2d SIGRTMIN".
        let mut zero = format!("{:2} SIGRTMIN{:<+3}", 34, 0);
        zero.truncate(11);
        assert_eq!(zero, "34 SIGRTMIN");
        // n > 0: keep the left-justified, forced-sign, width-3 suffix.
        assert_eq!(format!("{:2} SIGRTMIN{:<+3}", 35, 1), "35 SIGRTMIN+1 ");
        assert_eq!(format!("{:2} SIGRTMIN{:<+3}", 64, 30), "64 SIGRTMIN+30");
    }
}
