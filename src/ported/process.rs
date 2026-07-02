//! Partial port of `Process.c` — pure string/state helpers are ported;
//! everything requiring unported substrate (RichString, ncurses/CRT
//! attrs, Panel, Object vtables, syscalls, or `Process` struct fields)
//! remains a `todo!()` stub named after its real htop C function.
//!
//! C names are preserved verbatim, so `non_snake_case` is allowed for
//! the whole module. Ported so far: [`processStateChar`],
//! [`findCommInCmdline`], [`matchCmdlinePrefixWithExeSuffix`], and
//! [`skipPotentialPath`] — the pure `const char*` + `size_t` helpers,
//! modeled on `&[u8]` + `usize`. NUL-terminated C string reads are
//! modeled by treating any index at/after the slice length as the
//! terminating NUL (`0`), matching a well-formed C string's `s[len]`.
//! Out-params are returned as tuples/`Option`.
//!
//! Each remaining `todo!()` is a placeholder named after a real htop C
//! function so the port-purity gate accepts the module and the port
//! surface is laid out. `gen_port_report.py` counts these `todo!()`
//! bodies as *stubbed*, not *ported*, so scaffolding does not inflate
//! coverage.
#![allow(non_snake_case)]
#![allow(dead_code)]

/// Port of `enum ProcessState_` from `Process.h:41`. Discriminants match
/// the C enum exactly (`UNKNOWN = 1`, the rest ascending).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
#[allow(non_camel_case_types)]
pub enum ProcessState {
    UNKNOWN = 1,
    RUNNABLE,
    RUNNING,
    QUEUED,
    WAITING,
    UNINTERRUPTIBLE_WAIT,
    BLOCKED,
    PAGING,
    STOPPED,
    TRACED,
    ZOMBIE,
    DEFUNCT,
    IDLE,
    SLEEPING,
}

/// Port of `#define TASK_COMM_LEN 16` from `Process.c:65`.
const TASK_COMM_LEN: usize = 16;

/// Port of `findCommInCmdline(const char* comm, const char* cmdline,
/// size_t cmdlineBasenameStart, size_t* pCommStart, size_t* pCommLen)`
/// from `Process.c:67`. Tokenizes `cmdline` starting at
/// `cmdlineBasenameStart` (tokens split on `\n`, basename reset after
/// each `/`) and looks for a token whose basename equals `comm` — an
/// exact length match, or a longer token when `comm` is the max comm
/// length (`TASK_COMM_LEN - 1 == 15`, i.e. a truncated comm). Returns
/// `Some((commStart, commLen))` (the two C out-params) on the first
/// match, else `None`. `comm` and `cmdline` are byte slices with no
/// trailing NUL; the C `*token` end-of-string test maps to reaching the
/// slice end.
pub fn findCommInCmdline(
    comm: &[u8],
    cmdline: &[u8],
    cmdlineBasenameStart: usize,
) -> Option<(usize, usize)> {
    let commLen = comm.len();

    let mut token = cmdlineBasenameStart;
    while token < cmdline.len() {
        let mut tokenBase = token;
        while token < cmdline.len() && cmdline[token] != b'\n' {
            if cmdline[token] == b'/' {
                tokenBase = token + 1;
            }
            token += 1;
        }
        let tokenLen = token - tokenBase;

        if (tokenLen == commLen || (tokenLen > commLen && commLen == TASK_COMM_LEN - 1))
            && cmdline[tokenBase..tokenBase + commLen] == comm[..commLen]
        {
            return Some((tokenBase, tokenLen));
        }

        if token < cmdline.len() {
            loop {
                token += 1;
                if !(token < cmdline.len() && cmdline[token] == b'\n') {
                    break;
                }
            }
        }
    }
    None
}

/// Port of `matchCmdlinePrefixWithExeSuffix(const char* cmdline, size_t*
/// cmdlineBasenameStart, const char* exe, size_t exeBaseOffset, size_t
/// exeBaseLen)` from `Process.c:99`. Returns `(matchLen,
/// cmdlineBasenameStart)`: `matchLen` is the C return value (0 = no
/// match), and the second element is the (possibly adjusted) value of
/// the `*cmdlineBasenameStart` in/out-param — updated only on the
/// relative-path success path, otherwise the input passes through
/// unchanged. NUL-terminated reads are modeled as `0` for any index at
/// or beyond the slice length.
pub fn matchCmdlinePrefixWithExeSuffix(
    cmdline: &[u8],
    cmdlineBasenameStart: usize,
    exe: &[u8],
    exeBaseOffset: usize,
    exeBaseLen: usize,
) -> (usize, usize) {
    let at = |s: &[u8], i: usize| -> u8 {
        if i < s.len() {
            s[i]
        } else {
            0
        }
    };
    // strncmp(a+ao, b+bo, n) == 0 with C NUL semantics.
    let strncmp_eq = |a: &[u8], ao: usize, b: &[u8], bo: usize, n: usize| -> bool {
        for k in 0..n {
            let ca = if ao + k < a.len() { a[ao + k] } else { 0 };
            let cb = if bo + k < b.len() { b[bo + k] } else { 0 };
            if ca != cb {
                return false;
            }
            if ca == 0 {
                break;
            }
        }
        true
    };

    /* cmdline prefix is an absolute path: it must match whole exe. */
    if at(cmdline, 0) == b'/' {
        let matchLen = exeBaseLen + exeBaseOffset;
        if strncmp_eq(cmdline, 0, exe, 0, matchLen) {
            let delim = at(cmdline, matchLen);
            if delim == 0 || delim == b'\n' || delim == b' ' {
                return (matchLen, cmdlineBasenameStart);
            }
        }
        return (0, cmdlineBasenameStart);
    }

    /* cmdline prefix is a relative path: match the basename, then reverse
     * match the cmdline prefix with the exe suffix; if that fails, back
     * up to the previous cmdline path component and retry. */
    let mut cmdlineBaseOffset = cmdlineBasenameStart;
    let mut delimFound; /* if valid basename delimiter found */
    loop {
        /* match basename */
        let matchLen = exeBaseLen + cmdlineBaseOffset;
        if cmdlineBaseOffset < exeBaseOffset
            && strncmp_eq(cmdline, cmdlineBaseOffset, exe, exeBaseOffset, exeBaseLen)
        {
            let delim = at(cmdline, matchLen);
            if delim == 0 || delim == b'\n' || delim == b' ' {
                /* reverse match the cmdline prefix and exe suffix */
                let mut i = cmdlineBaseOffset;
                let mut j = exeBaseOffset;
                while i >= 1 && j >= 1 && at(cmdline, i - 1) == at(exe, j - 1) {
                    i -= 1;
                    j -= 1;
                }

                /* full match, with exe suffix being a valid relative path */
                if i < 1 && j >= 1 && at(exe, j - 1) == b'/' {
                    return (matchLen, cmdlineBaseOffset);
                }
            }
        }

        /* Try to find the previous potential cmdlineBaseOffset - it would
         * be preceded by '/' or nothing, and delimited by ' ' or '\n' */
        delimFound = false;
        if cmdlineBaseOffset <= 2 {
            return (0, cmdlineBasenameStart);
        }
        cmdlineBaseOffset -= 2;
        while cmdlineBaseOffset > 0 {
            if delimFound {
                if at(cmdline, cmdlineBaseOffset - 1) == b'/' {
                    break;
                }
            } else if at(cmdline, cmdlineBaseOffset) == b' '
                || at(cmdline, cmdlineBaseOffset) == b'\n'
            {
                delimFound = true;
            }
            cmdlineBaseOffset -= 1;
        }

        if !delimFound {
            return (0, cmdlineBasenameStart);
        }
    }
}

/// TODO: port of `void Process_fillStarttimeBuffer(Process* this` from `Process.c:43`.
pub fn Process_fillStarttimeBuffer() {
    todo!("port of Process.c:43")
}

/// TODO: port of `static inline char* stpcpyWithNewlineConversion(char* dstStr, const char* srcStr` from `Process.c:169`.
pub fn stpcpyWithNewlineConversion() {
    todo!("port of Process.c:169")
}

/// TODO: port of `void Process_makeCommandStr(Process* this, const Settings* settings` from `Process.c:183`.
pub fn Process_makeCommandStr() {
    todo!("port of Process.c:183")
}

/// TODO: port of `void Process_writeCommand(const Process* this, int attr, int baseAttr, RichString* str` from `Process.c:471`.
pub fn Process_writeCommand() {
    todo!("port of Process.c:471")
}

/// Port of `processStateChar(ProcessState state)` from `Process.c:545`.
/// Maps a [`ProcessState`] to its single-character display code. The C
/// `default: assert(0); return '!'` path is unreachable here — a valid
/// `ProcessState` value covers every arm — so the match is exhaustive.
pub fn processStateChar(state: ProcessState) -> char {
    match state {
        ProcessState::UNKNOWN => '?',
        ProcessState::RUNNABLE => 'U',
        ProcessState::RUNNING => 'R',
        ProcessState::QUEUED => 'Q',
        ProcessState::WAITING => 'W',
        ProcessState::UNINTERRUPTIBLE_WAIT => 'D',
        ProcessState::BLOCKED => 'B',
        ProcessState::PAGING => 'P',
        ProcessState::STOPPED => 'T',
        ProcessState::TRACED => 't',
        ProcessState::ZOMBIE => 'Z',
        ProcessState::DEFUNCT => 'X',
        ProcessState::IDLE => 'I',
        ProcessState::SLEEPING => 'S',
    }
}

/// TODO: port of `static void Process_rowWriteField(const Row* super, RichString* str, RowField field` from `Process.c:567`.
pub fn Process_rowWriteField() {
    todo!("port of Process.c:567")
}

/// TODO: port of `void Process_writeField(const Process* this, RichString* str, RowField field` from `Process.c:573`.
pub fn Process_writeField() {
    todo!("port of Process.c:573")
}

/// TODO: port of `void Process_done(Process* this` from `Process.c:795`.
pub fn Process_done() {
    todo!("port of Process.c:795")
}

/// TODO: port of `const char* Process_getCommand(const Process* this` from `Process.c:808`.
pub fn Process_getCommand() {
    todo!("port of Process.c:808")
}

/// TODO: port of `static const char* Process_getSortKey(const Process* this` from `Process.c:818`.
pub fn Process_getSortKey() {
    todo!("port of Process.c:818")
}

/// TODO: port of `const char* Process_rowGetSortKey(Row* super` from `Process.c:822`.
pub fn Process_rowGetSortKey() {
    todo!("port of Process.c:822")
}

/// TODO: port of `static bool Process_isHighlighted(const Process* this` from `Process.c:829`.
pub fn Process_isHighlighted() {
    todo!("port of Process.c:829")
}

/// TODO: port of `bool Process_rowIsHighlighted(const Row* super` from `Process.c:835`.
pub fn Process_rowIsHighlighted() {
    todo!("port of Process.c:835")
}

/// TODO: port of `static bool Process_isVisible(const Process* p, const Settings* settings` from `Process.c:842`.
pub fn Process_isVisible() {
    todo!("port of Process.c:842")
}

/// TODO: port of `bool Process_rowIsVisible(const Row* super, const Table* table` from `Process.c:848`.
pub fn Process_rowIsVisible() {
    todo!("port of Process.c:848")
}

/// TODO: port of `static bool Process_matchesFilter(const Process* this, const Table* table` from `Process.c:855`.
pub fn Process_matchesFilter() {
    todo!("port of Process.c:855")
}

/// TODO: port of `bool Process_rowMatchesFilter(const Row* super, const Table* table` from `Process.c:872`.
pub fn Process_rowMatchesFilter() {
    todo!("port of Process.c:872")
}

/// TODO: port of `void Process_init(Process* this, const Machine* host` from `Process.c:878`.
pub fn Process_init() {
    todo!("port of Process.c:878")
}

/// TODO: port of `static bool Process_setPriority(Process* this, int priority` from `Process.c:885`.
pub fn Process_setPriority() {
    todo!("port of Process.c:885")
}

/// TODO: port of `bool Process_rowChangePriorityBy(Row* super, Arg delta` from `Process.c:898`.
pub fn Process_rowChangePriorityBy() {
    todo!("port of Process.c:898")
}

/// TODO: port of `static bool Process_sendSignal(Process* this, Arg sgn` from `Process.c:904`.
pub fn Process_sendSignal() {
    todo!("port of Process.c:904")
}

/// TODO: port of `bool Process_rowSendSignal(Row* super, Arg sgn` from `Process.c:908`.
pub fn Process_rowSendSignal() {
    todo!("port of Process.c:908")
}

/// TODO: port of `int Process_compare(const void* v1, const void* v2` from `Process.c:914`.
pub fn Process_compare() {
    todo!("port of Process.c:914")
}

/// TODO: port of `int Process_compareByParent(const Row* r1, const Row* r2` from `Process.c:931`.
pub fn Process_compareByParent() {
    todo!("port of Process.c:931")
}

/// TODO: port of `int Process_compareByKey_Base(const Process* p1, const Process* p2, ProcessField key` from `Process.c:943`.
pub fn Process_compareByKey_Base() {
    todo!("port of Process.c:943")
}

/// TODO: port of `void Process_updateComm(Process* this, const char* comm` from `Process.c:1020`.
pub fn Process_updateComm() {
    todo!("port of Process.c:1020")
}

/// Port of `skipPotentialPath(const char* cmdline, size_t end)` from
/// `Process.c:1033`. If `cmdline` starts with `/`, scans up to `end`
/// bytes and returns the offset just past the last `/` that begins a
/// non-empty path component, stopping early at an unescaped space or a
/// `": "` delimiter. Returns 0 when `cmdline` is not an absolute path.
/// NUL-terminated reads are modeled as `0` for any index at or beyond
/// the slice length (the C `cmdline[i + 1]` NUL lookahead).
pub fn skipPotentialPath(cmdline: &[u8], end: usize) -> usize {
    let at = |i: usize| -> u8 {
        if i < cmdline.len() {
            cmdline[i]
        } else {
            0
        }
    };

    if at(0) != b'/' {
        return 0;
    }

    let mut slash = 0;
    let mut i = 1;
    while i < end {
        if at(i) == b'/' && at(i + 1) != 0 {
            slash = i + 1;
            i += 1;
            continue;
        }

        if at(i) == b' ' && at(i - 1) != b'\\' {
            return slash;
        }

        if at(i) == b':' && at(i + 1) == b' ' {
            return slash;
        }

        i += 1;
    }

    slash
}

/// TODO: port of `void Process_updateCmdline(Process* this, const char* cmdline, size_t basenameStart, size_t basenameEnd` from `Process.c:1054`.
pub fn Process_updateCmdline() {
    todo!("port of Process.c:1054")
}

/// TODO: port of `void Process_updateExe(Process* this, const char* exe` from `Process.c:1079`.
pub fn Process_updateExe() {
    todo!("port of Process.c:1079")
}

/// TODO: port of `void Process_updateCPUFieldWidths(float percentage` from `Process.c:1099`.
pub fn Process_updateCPUFieldWidths() {
    todo!("port of Process.c:1099")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_state_char_maps_every_state() {
        assert_eq!(processStateChar(ProcessState::UNKNOWN), '?');
        assert_eq!(processStateChar(ProcessState::RUNNABLE), 'U');
        assert_eq!(processStateChar(ProcessState::RUNNING), 'R');
        assert_eq!(processStateChar(ProcessState::QUEUED), 'Q');
        assert_eq!(processStateChar(ProcessState::WAITING), 'W');
        assert_eq!(processStateChar(ProcessState::UNINTERRUPTIBLE_WAIT), 'D');
        assert_eq!(processStateChar(ProcessState::BLOCKED), 'B');
        assert_eq!(processStateChar(ProcessState::PAGING), 'P');
        assert_eq!(processStateChar(ProcessState::STOPPED), 'T');
        assert_eq!(processStateChar(ProcessState::TRACED), 't');
        assert_eq!(processStateChar(ProcessState::ZOMBIE), 'Z');
        assert_eq!(processStateChar(ProcessState::DEFUNCT), 'X');
        assert_eq!(processStateChar(ProcessState::IDLE), 'I');
        assert_eq!(processStateChar(ProcessState::SLEEPING), 'S');
    }

    #[test]
    fn process_state_discriminants_match_c() {
        // C: UNKNOWN = 1, the rest ascending (Process.h:41).
        assert_eq!(ProcessState::UNKNOWN as u8, 1);
        assert_eq!(ProcessState::SLEEPING as u8, 14);
    }

    #[test]
    fn find_comm_exact_token_match() {
        // Tokens split on '\n' (the C inner loop breaks only on '\n');
        // cmdlineBasenameStart points at 'b' of "bash" (index 9).
        let cmdline = b"/usr/bin/bash\n--login";
        assert_eq!(findCommInCmdline(b"bash", cmdline, 9), Some((9, 4)));
    }

    #[test]
    fn find_comm_resets_basename_after_slash() {
        // Starting the scan before a slash: tokenBase resets past '/'.
        let cmdline = b"/usr/bin/bash";
        assert_eq!(findCommInCmdline(b"bash", cmdline, 0), Some((9, 4)));
    }

    #[test]
    fn find_comm_no_match_returns_none() {
        let cmdline = b"/usr/bin/zsh";
        assert_eq!(findCommInCmdline(b"bash", cmdline, 0), None);
        // empty cmdline: loop never enters.
        assert_eq!(findCommInCmdline(b"bash", b"", 0), None);
    }

    #[test]
    fn find_comm_truncated_comm_allows_longer_token() {
        // commLen == TASK_COMM_LEN - 1 (15): a longer token still matches
        // on its 15-char prefix.
        let comm = b"012345678901234"; // 15 bytes
        assert_eq!(comm.len(), TASK_COMM_LEN - 1);
        let cmdline = b"0123456789012345678"; // 19 bytes, prefix matches
        assert_eq!(findCommInCmdline(comm, cmdline, 0), Some((0, 19)));
        // With a comm of non-max length, a longer token must NOT match.
        assert_eq!(findCommInCmdline(b"0123", b"01234567", 0), None);
    }

    #[test]
    fn find_comm_skips_consecutive_newlines() {
        // tokens split on '\n'; multiple newlines are collapsed.
        let cmdline = b"foo\n\n\nbar";
        assert_eq!(findCommInCmdline(b"bar", cmdline, 0), Some((6, 3)));
    }

    #[test]
    fn skip_potential_path_non_absolute_returns_zero() {
        assert_eq!(skipPotentialPath(b"bash --login", 12), 0);
        assert_eq!(skipPotentialPath(b"", 0), 0);
    }

    #[test]
    fn skip_potential_path_returns_after_last_slash() {
        // "/usr/bin/bash" -> offset just past the last '/' (index 9).
        let c = b"/usr/bin/bash";
        assert_eq!(skipPotentialPath(c, c.len()), 9);
    }

    #[test]
    fn skip_potential_path_stops_at_unescaped_space() {
        // "/usr/bin/bash --login": scanning stops at the space; the last
        // component slash was at index 9.
        let c = b"/usr/bin/bash --login";
        assert_eq!(skipPotentialPath(c, c.len()), 9);
    }

    #[test]
    fn skip_potential_path_escaped_space_does_not_stop() {
        // Escaped space (preceded by '\\') is not a delimiter, so the
        // scan continues past it to the final "/d" component (slash = 8).
        let c = b"/a/b\\ c/d";
        assert_eq!(skipPotentialPath(c, c.len()), 8);
    }

    #[test]
    fn skip_potential_path_stops_at_colon_space() {
        // ": " delimiter stops the scan.
        let c = b"/usr/bin/foo: bar";
        assert_eq!(skipPotentialPath(c, c.len()), 9);
    }

    #[test]
    fn skip_potential_path_trailing_slash_not_counted() {
        // A '/' whose next byte is NUL (end of slice) does not advance
        // slash: cmdline[i + 1] != '\0' guard fails.
        let c = b"/usr/bin/";
        assert_eq!(skipPotentialPath(c, c.len()), 5);
    }

    #[test]
    fn match_exe_absolute_path_full_match() {
        // exe = "/usr/bin/bash", exeBaseOffset = 9 ("bash" at 9),
        // exeBaseLen = 4. Absolute cmdline must match the whole exe.
        let exe = b"/usr/bin/bash";
        let cmdline = b"/usr/bin/bash --login";
        let (matchLen, base) = matchCmdlinePrefixWithExeSuffix(cmdline, 9, exe, 9, 4);
        assert_eq!(matchLen, 13); // exeBaseLen + exeBaseOffset
        assert_eq!(base, 9); // unchanged on absolute path
    }

    #[test]
    fn match_exe_absolute_path_no_match() {
        let exe = b"/usr/bin/bash";
        let cmdline = b"/usr/bin/zsh";
        let (matchLen, base) = matchCmdlinePrefixWithExeSuffix(cmdline, 9, exe, 9, 3);
        assert_eq!(matchLen, 0);
        assert_eq!(base, 9);
    }

    #[test]
    fn match_exe_absolute_bad_delimiter() {
        // cmdline continues the basename past the matched prefix with a
        // non-delimiter char, so the match is rejected.
        let exe = b"/usr/bin/bash";
        let cmdline = b"/usr/bin/bashx";
        let (matchLen, _) = matchCmdlinePrefixWithExeSuffix(cmdline, 9, exe, 9, 4);
        assert_eq!(matchLen, 0);
    }

    #[test]
    fn match_exe_relative_path_reverse_match() {
        // exe = "/usr/bin/bash" (basename "bash" at offset 9), cmdline is
        // the relative "bin/bash" with basename "bash" at offset 4. The
        // reverse match walks "bin/" back to exe's "/bin/".
        let exe = b"/usr/bin/bash";
        let cmdline = b"bin/bash";
        let (matchLen, base) = matchCmdlinePrefixWithExeSuffix(cmdline, 4, exe, 9, 4);
        assert_eq!(matchLen, 8); // exeBaseLen(4) + cmdlineBaseOffset(4)
        assert_eq!(base, 4);
    }

    #[test]
    fn match_exe_relative_no_match() {
        let exe = b"/usr/bin/bash";
        let cmdline = b"bin/zsh";
        let (matchLen, base) = matchCmdlinePrefixWithExeSuffix(cmdline, 4, exe, 9, 3);
        assert_eq!(matchLen, 0);
        assert_eq!(base, 4); // in-param passes through unchanged
    }
}
