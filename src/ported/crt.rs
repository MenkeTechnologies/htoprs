//! Stub scaffold for `CRT.c` — NOT yet ported.
//!
//! Every `pub fn` below is a placeholder (`todo!()`) named after a real
//! htop C function so the port-purity gate accepts the module and the
//! port surface is laid out. Replace each stub with a faithful port of
//! the C body, updating the signature and the doc comment to `Port of
//! `CRT.c`:<line>.` as you go. `gen_port_report.py` counts these
//! `todo!()` bodies as *stubbed*, not *ported*, so scaffolding does not
//! inflate coverage.
#![allow(non_snake_case)]
#![allow(dead_code)]


/// TODO: port of `static void initDegreeSign(void` from `CRT.c:109`.
pub fn initDegreeSign() {
    todo!("port of CRT.c:109")
}

/// TODO: port of `static void CRT_handleSIGTERM(int sgn` from `CRT.c:961`.
pub fn CRT_handleSIGTERM() {
    todo!("port of CRT.c:961")
}

/// TODO: port of `static int createStderrCacheFile(void` from `CRT.c:984`.
pub fn createStderrCacheFile() {
    todo!("port of CRT.c:984")
}

/// TODO: port of `static void redirectStderr(void` from `CRT.c:1003`.
pub fn redirectStderr() {
    todo!("port of CRT.c:1003")
}

/// TODO: port of `static void dumpStderr(void` from `CRT.c:1014`.
pub fn dumpStderr() {
    todo!("port of CRT.c:1014")
}

/// TODO: port of `void CRT_debug_impl(const char* file, size_t lineno, const char* func, const char* fmt, ...` from `CRT.c:1056`.
pub fn CRT_debug_impl() {
    todo!("port of CRT.c:1056")
}

/// TODO: port of `static void CRT_installSignalHandlers(void` from `CRT.c:1078`.
pub fn CRT_installSignalHandlers() {
    todo!("port of CRT.c:1078")
}

/// TODO: port of `void CRT_resetSignalHandlers(void` from `CRT.c:1103`.
pub fn CRT_resetSignalHandlers() {
    todo!("port of CRT.c:1103")
}

/// TODO: port of `void CRT_setMouse(bool enabled` from `CRT.c:1120`.
pub fn CRT_setMouse() {
    todo!("port of CRT.c:1120")
}

/// Port of `CRT.c:1133`.
///
/// Determines whether a given `TERM` value is one for which htop trusts
/// terminfo's defined-key set (so it does not need to install its own key
/// definitions). `termType` maps the C `const char* termType` which may be
/// `NULL`; `None` reproduces the `!termType` early-return `false`.
///
/// The C body indexes past `String_startsWith`/`String_eq` matches (e.g.
/// `termType[6]` after matching `"screen"`); reading a byte at or beyond the
/// terminating NUL yields `'\0'` in C, modelled here with
/// `.get(i).copied().unwrap_or(0)`. `IS_END_OR_DASH(ch)` is `ch == '-' || ch == '\0'`.
pub fn terminalSupportsDefinedKeys(termType: Option<&str>) -> bool {
    let termType = match termType {
        None => return false,
        Some(s) => s,
    };

    let bytes = termType.as_bytes();
    // Byte at index `i`, or 0 ('\0') at/after the C NUL terminator.
    let at = |i: usize| -> u8 { bytes.get(i).copied().unwrap_or(0) };
    // #define IS_END_OR_DASH(ch) ((ch) == '-' || (ch) == '\0')
    let is_end_or_dash = |ch: u8| ch == b'-' || ch == b'\0';

    match at(0) {
        b'a' => termType == "alacritty",
        b'f' => termType == "foot",
        b's' => {
            if at(1) == b't' && is_end_or_dash(at(2)) {
                return true;
            }
            if termType.starts_with("screen") && is_end_or_dash(at(6)) {
                return true;
            }
            false
        }
        b't' => termType.starts_with("tmux") && is_end_or_dash(at(4)),
        b'v' => termType == "vt220",
        b'x' => termType.starts_with("xterm") && is_end_or_dash(at(5)),
        _ => false,
    }
}

/// TODO: port of `void CRT_init(const Settings* settings, bool allowUnicode, bool retainScreenOnExit` from `CRT.c:1179`.
pub fn CRT_init() {
    todo!("port of CRT.c:1179")
}

/// TODO: port of `void CRT_done(void` from `CRT.c:1290`.
pub fn CRT_done() {
    todo!("port of CRT.c:1290")
}

/// TODO: port of `void CRT_fatalError(const char* note` from `CRT.c:1308`.
pub fn CRT_fatalError() {
    todo!("port of CRT.c:1308")
}

/// TODO: port of `int CRT_readKey(void` from `CRT.c:1315`.
pub fn CRT_readKey() {
    todo!("port of CRT.c:1315")
}

/// TODO: port of `void CRT_disableDelay(void` from `CRT.c:1324`.
pub fn CRT_disableDelay() {
    todo!("port of CRT.c:1324")
}

/// TODO: port of `void CRT_enableDelay(void` from `CRT.c:1330`.
pub fn CRT_enableDelay() {
    todo!("port of CRT.c:1330")
}

/// TODO: port of `void CRT_setColors(int colorScheme` from `CRT.c:1334`.
pub fn CRT_setColors() {
    todo!("port of CRT.c:1334")
}

/// TODO: port of `static void print_backtrace(void` from `CRT.c:1360`.
pub fn print_backtrace() {
    todo!("port of CRT.c:1360")
}

/// TODO: port of `void CRT_handleSIGSEGV(int signal` from `CRT.c:1420`.
pub fn CRT_handleSIGSEGV() {
    todo!("port of CRT.c:1420")
}

#[cfg(test)]
mod tests {
    use super::terminalSupportsDefinedKeys;

    #[test]
    fn null_term_is_false() {
        // C: `if (!termType) return false;`
        assert!(!terminalSupportsDefinedKeys(None));
    }

    #[test]
    fn exact_match_terminals() {
        // 'a'/'f'/'v' arms require full String_eq, not a prefix.
        assert!(terminalSupportsDefinedKeys(Some("alacritty")));
        assert!(terminalSupportsDefinedKeys(Some("foot")));
        assert!(terminalSupportsDefinedKeys(Some("vt220")));

        // Right first char but not the full string => break => false.
        assert!(!terminalSupportsDefinedKeys(Some("alacritt")));
        assert!(!terminalSupportsDefinedKeys(Some("alacritty-256")));
        assert!(!terminalSupportsDefinedKeys(Some("footloose")));
        assert!(!terminalSupportsDefinedKeys(Some("vt100")));
    }

    #[test]
    fn st_terminal_and_dash_boundary() {
        // termType[1] == 't' && IS_END_OR_DASH(termType[2])
        assert!(terminalSupportsDefinedKeys(Some("st")));
        assert!(terminalSupportsDefinedKeys(Some("st-256color")));
        // "st" branch fails when [2] is neither '-' nor '\0'.
        assert!(!terminalSupportsDefinedKeys(Some("sti")));
        // 's' but not 't' at [1], and not "screen*".
        assert!(!terminalSupportsDefinedKeys(Some("sun")));
        // Lone "s": [1] is '\0' != 't', screen check false.
        assert!(!terminalSupportsDefinedKeys(Some("s")));
    }

    #[test]
    fn screen_prefix_with_dash_boundary() {
        // String_startsWith(termType, "screen") && IS_END_OR_DASH(termType[6])
        assert!(terminalSupportsDefinedKeys(Some("screen")));
        assert!(terminalSupportsDefinedKeys(Some("screen-256color")));
        // startsWith("screen") true but [6] is a letter, not end/dash.
        assert!(!terminalSupportsDefinedKeys(Some("screensaver")));
        assert!(!terminalSupportsDefinedKeys(Some("scr")));
    }

    #[test]
    fn tmux_and_xterm_prefix_boundary() {
        assert!(terminalSupportsDefinedKeys(Some("tmux")));
        assert!(terminalSupportsDefinedKeys(Some("tmux-256color")));
        assert!(!terminalSupportsDefinedKeys(Some("tmuxx")));

        assert!(terminalSupportsDefinedKeys(Some("xterm")));
        assert!(terminalSupportsDefinedKeys(Some("xterm-256color")));
        assert!(!terminalSupportsDefinedKeys(Some("xterms")));
    }

    #[test]
    fn empty_and_unknown_first_char() {
        // Empty string: switch on '\0' hits default => false.
        assert!(!terminalSupportsDefinedKeys(Some("")));
        // First char with no arm.
        assert!(!terminalSupportsDefinedKeys(Some("linux")));
        assert!(!terminalSupportsDefinedKeys(Some("dumb")));
    }
}
