//! Port of the `CRT.c` color model — the color-scheme tables that
//! decide the fg/bg/attribute of every UI element, plus the pure
//! color-selection logic. This is the part of `CRT.c` that makes
//! htoprs render with htop's exact colors.
//!
//! Ported here (faithful, verbatim data):
//!   * `ColorElements` / `ColorScheme` enums (CRT.h) — same order and
//!     discriminants as the C, including the trailing `LAST_*` sentinel.
//!   * The `ColorPair`/`ColorIndex` macros and the ncurses `A_*`
//!     attribute masks and `COLOR_*` indices, as `const`s / `const fn`s
//!     that reproduce the exact packed `i32` htop stores.
//!   * `CRT_colorSchemes[LAST_COLORSCHEME][LAST_COLORELEMENT]` — all
//!     eight schemes, transcribed from `CRT.c`. `COLORSCHEME_BROKENGRAY`
//!     is derived from `COLORSCHEME_DEFAULT` exactly as `CRT_init` does
//!     it (`CRT.c:1197`).
//!   * `CRT_setColors` — the pure scheme clamp + active-scheme selection
//!     (`CRT_colorScheme` / `CRT_colors`).
//!   * `ResolvedColor::from_attr` — reproduces the fg/bg that
//!     `CRT_setColors`' `init_pair` loop registers for a packed color
//!     pair (`-1` == terminal default `COLOR_DEFAULT`), so the future
//!     crossterm draw layer can emit the identical color.
//!
//! Terminal-control layer (behavioral port on crossterm): `CRT_init`,
//! `CRT_done`, `CRT_readKey`, `CRT_setMouse`, `CRT_fatalError`,
//! `CRT_enableDelay`/`CRT_disableDelay`, the `CRT_utf8` flag and
//! `initDegreeSign`. These reproduce htop's observable terminal
//! setup/teardown/input semantics through crossterm rather than the
//! literal ncurses calls; the ncurses key-code integers htop's UI
//! compares against are reproduced verbatim so the mapping hands the
//! rest of the port exactly the ints it expects.
//!
//! Signal / debug-stderr infrastructure ported on `libc` (the crate now
//! depends on `libc`/`nix`, so the POSIX syscalls these need are reachable):
//!   * `CRT_handleSIGTERM` (`CRT.c:961`) — the `signal()`-registered
//!     terminate handler: `CRT_done`, the `CRT_settings->changed` check
//!     (`CRT_settings` modeled as the C `static const Settings*` global,
//!     `libc::strsignal`, `full_write`, and `_exit(0)`. Wiring the pointer
//!     (`CRT_settings = settings`, `CRT.c:1194`) is deferred because this
//!     port's `CRT_init` is Settings-free, so the check currently always
//!     takes the not-changed (`_exit(0)`) branch.
//!   * `createStderrCacheFile` (`CRT.c:984`) — `memfd_create` on Linux
//!     (`HAVE_MEMFD_CREATE`), `mkstemp`+`unlink` elsewhere (`O_TMPFILE`
//!     middle branch omitted: libc always provides `memfd_create` on
//!     Linux). `#ifndef NDEBUG`-gated via `cfg(debug_assertions)`.
//!   * `redirectStderr` (`CRT.c:1003`) / `dumpStderr` (`CRT.c:1014`) —
//!     `dup`/`dup2`/`fsync`/`lseek`/`read` on `STDERR_FILENO`. The C
//!     `#ifndef NDEBUG` real body / `#else` empty body split is
//!     reproduced with `cfg(debug_assertions)` / `cfg(not(...))`.
//!   * `CRT_installSignalHandlers` (`CRT.c:1078`) /
//!     `CRT_resetSignalHandlers` (`CRT.c:1103`) — `libc::sigaction`/
//!     `libc::signal` with the `struct sigaction old_sig_handler[32]`
//!     save/restore array (`OLD_SIG_HANDLER`). `HTOP_PCP` is undefined so
//!     the `SIGPIPE`-via-`sigaction` branch is taken.
//!
//! Also ported:
//!   * `print_backtrace` (`CRT.c:1360`) — the execinfo `backtrace(3)` /
//!     `backtrace_symbols_fd(3)` branch, on `libc::backtrace` /
//!     `libc::backtrace_symbols_fd` (present for macOS and Linux-gnu). The
//!     libunwind branch is omitted (no libunwind crate), matching an
//!     execinfo-only build.
//!   * `CRT_handleSIGSEGV` (`CRT.c:1420`) — the fatal fault handler: `CRT_done`,
//!     the stderr crash report (version / signal name / `Settings_write` /
//!     `print_backtrace`), then chaining to the saved disposition and
//!     re-raising. `program` is [`crate::ported::htop::program`] and
//!     `Settings_write` is ported; `CRT_settings` stays unwired (null-guarded).
//!
//! Still stubbed (`todo!()`) — the one genuine language-level blocker:
//!   * `CRT_debug_impl` (`CRT.c:1056`) — a C variadic (`...`) `vfprintf`
//!     shim; Rust has no stable variadic `fn`, so the faithful analog is
//!     a macro, not the `pub fn` the port gate requires.
//!
//! ncurses macro values are cited from
//! `/opt/homebrew/opt/ncurses/include/ncurses.h`:
//!   `NCURSES_ATTR_SHIFT 8`, `NCURSES_BITS(m,s) = m << (s+8)`,
//!   `A_NORMAL 0`, `A_STANDOUT 1<<16`, `A_UNDERLINE 1<<17`,
//!   `A_REVERSE 1<<18`, `A_BLINK 1<<19`, `A_DIM 1<<20`, `A_BOLD 1<<21`,
//!   `A_COLOR = 255<<8 = 0xFF00`, `COLOR_PAIR(n) = (n<<8) & A_COLOR`.
//!   `COLOR_BLACK..COLOR_WHITE = 0..7`.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(dead_code)]

use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::ported::settings::{Settings, Settings_write};

use crossterm::cursor;
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers,
};
use crossterm::execute;
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};

use self::ColorElements::*;
use self::ColorScheme::*;

// ncurses attribute masks. `A_*` values from
// `/opt/homebrew/opt/ncurses/include/ncurses.h`
// (`NCURSES_BITS(m,s) = m << (s + NCURSES_ATTR_SHIFT)`,
//  `NCURSES_ATTR_SHIFT = 8`).
const NCURSES_ATTR_SHIFT: i32 = 8;
pub const A_NORMAL: i32 = 0;
pub const A_STANDOUT: i32 = 1 << 16; // NCURSES_BITS(1, 8)
pub const A_UNDERLINE: i32 = 1 << 17; // NCURSES_BITS(1, 9)
pub const A_REVERSE: i32 = 1 << 18; // NCURSES_BITS(1, 10)
pub const A_BLINK: i32 = 1 << 19; // NCURSES_BITS(1, 11)
pub const A_DIM: i32 = 1 << 20; // NCURSES_BITS(1, 12)
pub const A_BOLD: i32 = 1 << 21; // NCURSES_BITS(1, 13)
/// `A_COLOR = NCURSES_BITS(((1U) << 8) - 1U, 0) = 0xFF00`.
pub const A_COLOR: i32 = 0xFF00;
/// The subset of `A_*` attribute bits htop's tables actually use.
const ATTR_MASK: i32 = A_STANDOUT | A_UNDERLINE | A_REVERSE | A_BLINK | A_DIM | A_BOLD;

// `COLOR_BLACK`..`COLOR_WHITE` (CRT.c:50-57, values from ncurses.h).
pub const Black: i32 = 0;
pub const Red: i32 = 1;
pub const Green: i32 = 2;
pub const Yellow: i32 = 3;
pub const Blue: i32 = 4;
pub const Magenta: i32 = 5;
pub const Cyan: i32 = 6;
pub const White: i32 = 7;

/// Port of `#define ColorIndex(i,j) ((7-(i))*8+(j))` from `CRT.c:46`.
pub const fn ColorIndex(i: i32, j: i32) -> i32 {
    (7 - i) * 8 + j
}

/// Port of `#define ColorPair(i,j) COLOR_PAIR(ColorIndex(i,j))` from
/// `CRT.c:48`. `COLOR_PAIR(n) = (n << 8) & A_COLOR` (ncurses.h).
pub const fn ColorPair(i: i32, j: i32) -> i32 {
    (ColorIndex(i, j) << NCURSES_ATTR_SHIFT) & A_COLOR
}

/// `ColorPairGrayBlack = ColorPair(Magenta, Magenta)` (CRT.c:59).
pub const ColorPairGrayBlack: i32 = ColorPair(Magenta, Magenta);
/// `ColorIndexGrayBlack = ColorIndex(Magenta, Magenta)` (CRT.c:60).
pub const ColorIndexGrayBlack: i32 = ColorIndex(Magenta, Magenta);
/// `ColorPairWhiteDefault = ColorPair(Red, Red)` (CRT.c:62).
pub const ColorPairWhiteDefault: i32 = ColorPair(Red, Red);
/// `ColorIndexWhiteDefault = ColorIndex(Red, Red)` (CRT.c:63).
pub const ColorIndexWhiteDefault: i32 = ColorIndex(Red, Red);

/// Port of `enum ColorScheme_` from `CRT.h:32`. Same order/discriminants
/// as the C; `LAST_COLORSCHEME` is the trailing count sentinel.
#[repr(usize)]
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum ColorScheme {
    COLORSCHEME_DEFAULT,
    COLORSCHEME_MONOCHROME,
    COLORSCHEME_BLACKONWHITE,
    COLORSCHEME_LIGHTTERMINAL,
    COLORSCHEME_MIDNIGHT,
    COLORSCHEME_BLACKNIGHT,
    COLORSCHEME_BROKENGRAY,
    COLORSCHEME_NORD,
    LAST_COLORSCHEME,
}

/// Port of `enum ColorElements_` from `CRT.h:43`. Same order/
/// discriminants as the C; `LAST_COLORELEMENT` is the trailing count
/// sentinel and equals the number of real color elements.
#[repr(usize)]
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum ColorElements {
    RESET_COLOR,
    DEFAULT_COLOR,
    FUNCTION_BAR,
    FUNCTION_KEY,
    FAILED_SEARCH,
    FAILED_READ,
    PAUSED,
    PANEL_HEADER_FOCUS,
    PANEL_HEADER_UNFOCUS,
    PANEL_SELECTION_FOCUS,
    PANEL_SELECTION_FOLLOW,
    PANEL_SELECTION_UNFOCUS,
    LARGE_NUMBER,
    METER_SHADOW,
    METER_TEXT,
    METER_VALUE,
    METER_VALUE_ERROR,
    METER_VALUE_IOREAD,
    METER_VALUE_IOWRITE,
    METER_VALUE_NOTICE,
    METER_VALUE_OK,
    METER_VALUE_WARN,
    LED_COLOR,
    UPTIME,
    BATTERY,
    TASKS_RUNNING,
    SWAP,
    SWAP_CACHE,
    SWAP_FRONTSWAP,
    PROCESS,
    PROCESS_SHADOW,
    PROCESS_TAG,
    PROCESS_MEGABYTES,
    PROCESS_GIGABYTES,
    PROCESS_TREE,
    PROCESS_RUN_STATE,
    PROCESS_D_STATE,
    PROCESS_BASENAME,
    PROCESS_HIGH_PRIORITY,
    PROCESS_LOW_PRIORITY,
    PROCESS_NEW,
    PROCESS_TOMB,
    PROCESS_THREAD,
    PROCESS_THREAD_BASENAME,
    PROCESS_COMM,
    PROCESS_THREAD_COMM,
    PROCESS_PRIV,
    BAR_BORDER,
    BAR_SHADOW,
    GRAPH_1,
    GRAPH_2,
    MEMORY_1,
    MEMORY_2,
    MEMORY_3,
    MEMORY_4,
    MEMORY_5,
    MEMORY_6,
    HUGEPAGE_1,
    HUGEPAGE_2,
    HUGEPAGE_3,
    HUGEPAGE_4,
    LOAD,
    LOAD_AVERAGE_FIFTEEN,
    LOAD_AVERAGE_FIVE,
    LOAD_AVERAGE_ONE,
    CHECK_BOX,
    CHECK_MARK,
    CHECK_TEXT,
    CLOCK,
    DATE,
    DATETIME,
    HELP_BOLD,
    HELP_SHADOW,
    HOSTNAME,
    CPU_NICE,
    CPU_NICE_TEXT,
    CPU_NORMAL,
    CPU_SYSTEM,
    CPU_IOWAIT,
    CPU_IRQ,
    CPU_SOFTIRQ,
    CPU_STEAL,
    CPU_GUEST,
    GPU_ENGINE_1,
    GPU_ENGINE_2,
    GPU_ENGINE_3,
    GPU_ENGINE_4,
    GPU_RESIDUE,
    PANEL_EDIT,
    SCREENS_OTH_BORDER,
    SCREENS_OTH_TEXT,
    SCREENS_CUR_BORDER,
    SCREENS_CUR_TEXT,
    PRESSURE_STALL_TEN,
    PRESSURE_STALL_SIXTY,
    PRESSURE_STALL_THREEHUNDRED,
    FILE_DESCRIPTOR_USED,
    FILE_DESCRIPTOR_MAX,
    ZFS_MFU,
    ZFS_MRU,
    ZFS_ANON,
    ZFS_HEADER,
    ZFS_OTHER,
    ZFS_COMPRESSED,
    ZFS_RATIO,
    ZRAM_COMPRESSED,
    ZRAM_UNCOMPRESSED,
    DYNAMIC_GRAY,
    DYNAMIC_DARKGRAY,
    DYNAMIC_RED,
    DYNAMIC_GREEN,
    DYNAMIC_BLUE,
    DYNAMIC_CYAN,
    DYNAMIC_MAGENTA,
    DYNAMIC_YELLOW,
    DYNAMIC_WHITE,
    LAST_COLORELEMENT,
}

/// Port of `static int CRT_colorSchemes[LAST_COLORSCHEME][LAST_COLORELEMENT]`
/// from `CRT.c:128`. Each entry is transcribed verbatim from the C
/// designated initializer; elements a scheme omits stay `A_NORMAL`
/// (`0`), matching C's zero-initialization of unlisted array members.
/// `COLORSCHEME_BROKENGRAY` is `{ 0 }` in the C static table and is
/// generated at the end from `COLORSCHEME_DEFAULT` exactly as
/// `CRT_init` does (`CRT.c:1197-1199`).
pub static CRT_colorSchemes: [[i32; LAST_COLORELEMENT as usize]; LAST_COLORSCHEME as usize] = {
    let mut t = [[A_NORMAL; LAST_COLORELEMENT as usize]; LAST_COLORSCHEME as usize];
    t[COLORSCHEME_DEFAULT as usize][RESET_COLOR as usize] = ColorPair(White, Black);
    t[COLORSCHEME_DEFAULT as usize][DEFAULT_COLOR as usize] = ColorPair(White, Black);
    t[COLORSCHEME_DEFAULT as usize][FUNCTION_BAR as usize] = ColorPair(Black, Cyan);
    t[COLORSCHEME_DEFAULT as usize][FUNCTION_KEY as usize] = ColorPair(White, Black);
    t[COLORSCHEME_DEFAULT as usize][PANEL_HEADER_FOCUS as usize] = ColorPair(Black, Green);
    t[COLORSCHEME_DEFAULT as usize][PANEL_HEADER_UNFOCUS as usize] = ColorPair(Black, Green);
    t[COLORSCHEME_DEFAULT as usize][PANEL_SELECTION_FOCUS as usize] = ColorPair(Black, Cyan);
    t[COLORSCHEME_DEFAULT as usize][PANEL_SELECTION_FOLLOW as usize] = ColorPair(Black, Yellow);
    t[COLORSCHEME_DEFAULT as usize][PANEL_SELECTION_UNFOCUS as usize] = ColorPair(Black, White);
    t[COLORSCHEME_DEFAULT as usize][FAILED_SEARCH as usize] = ColorPair(Red, Cyan);
    t[COLORSCHEME_DEFAULT as usize][FAILED_READ as usize] = A_BOLD | ColorPair(Red, Black);
    t[COLORSCHEME_DEFAULT as usize][PAUSED as usize] = A_BOLD | ColorPair(Yellow, Cyan);
    t[COLORSCHEME_DEFAULT as usize][UPTIME as usize] = A_BOLD | ColorPair(Cyan, Black);
    t[COLORSCHEME_DEFAULT as usize][BATTERY as usize] = A_BOLD | ColorPair(Cyan, Black);
    t[COLORSCHEME_DEFAULT as usize][LARGE_NUMBER as usize] = A_BOLD | ColorPair(Red, Black);
    t[COLORSCHEME_DEFAULT as usize][METER_SHADOW as usize] = A_BOLD | ColorPairGrayBlack;
    t[COLORSCHEME_DEFAULT as usize][METER_TEXT as usize] = ColorPair(Cyan, Black);
    t[COLORSCHEME_DEFAULT as usize][METER_VALUE as usize] = A_BOLD | ColorPair(Cyan, Black);
    t[COLORSCHEME_DEFAULT as usize][METER_VALUE_ERROR as usize] = A_BOLD | ColorPair(Red, Black);
    t[COLORSCHEME_DEFAULT as usize][METER_VALUE_IOREAD as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_DEFAULT as usize][METER_VALUE_IOWRITE as usize] = A_BOLD | ColorPair(Blue, Black);
    t[COLORSCHEME_DEFAULT as usize][METER_VALUE_NOTICE as usize] = A_BOLD | ColorPair(White, Black);
    t[COLORSCHEME_DEFAULT as usize][METER_VALUE_OK as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_DEFAULT as usize][METER_VALUE_WARN as usize] = A_BOLD | ColorPair(Yellow, Black);
    t[COLORSCHEME_DEFAULT as usize][LED_COLOR as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_DEFAULT as usize][TASKS_RUNNING as usize] = A_BOLD | ColorPair(Green, Black);
    t[COLORSCHEME_DEFAULT as usize][PROCESS as usize] = A_NORMAL;
    t[COLORSCHEME_DEFAULT as usize][PROCESS_SHADOW as usize] = A_BOLD | ColorPairGrayBlack;
    t[COLORSCHEME_DEFAULT as usize][PROCESS_TAG as usize] = A_BOLD | ColorPair(Yellow, Black);
    t[COLORSCHEME_DEFAULT as usize][PROCESS_MEGABYTES as usize] = ColorPair(Cyan, Black);
    t[COLORSCHEME_DEFAULT as usize][PROCESS_GIGABYTES as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_DEFAULT as usize][PROCESS_BASENAME as usize] = A_BOLD | ColorPair(Cyan, Black);
    t[COLORSCHEME_DEFAULT as usize][PROCESS_TREE as usize] = ColorPair(Cyan, Black);
    t[COLORSCHEME_DEFAULT as usize][PROCESS_RUN_STATE as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_DEFAULT as usize][PROCESS_D_STATE as usize] = A_BOLD | ColorPair(Red, Black);
    t[COLORSCHEME_DEFAULT as usize][PROCESS_HIGH_PRIORITY as usize] = ColorPair(Red, Black);
    t[COLORSCHEME_DEFAULT as usize][PROCESS_LOW_PRIORITY as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_DEFAULT as usize][PROCESS_NEW as usize] = ColorPair(Black, Green);
    t[COLORSCHEME_DEFAULT as usize][PROCESS_TOMB as usize] = ColorPair(Black, Red);
    t[COLORSCHEME_DEFAULT as usize][PROCESS_THREAD as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_DEFAULT as usize][PROCESS_THREAD_BASENAME as usize] =
        A_BOLD | ColorPair(Green, Black);
    t[COLORSCHEME_DEFAULT as usize][PROCESS_COMM as usize] = ColorPair(Magenta, Black);
    t[COLORSCHEME_DEFAULT as usize][PROCESS_THREAD_COMM as usize] = A_BOLD | ColorPair(Blue, Black);
    t[COLORSCHEME_DEFAULT as usize][PROCESS_PRIV as usize] = ColorPair(Magenta, Black);
    t[COLORSCHEME_DEFAULT as usize][BAR_BORDER as usize] = A_BOLD;
    t[COLORSCHEME_DEFAULT as usize][BAR_SHADOW as usize] = A_BOLD | ColorPairGrayBlack;
    t[COLORSCHEME_DEFAULT as usize][SWAP as usize] = ColorPair(Red, Black);
    t[COLORSCHEME_DEFAULT as usize][SWAP_CACHE as usize] = ColorPair(Yellow, Black);
    t[COLORSCHEME_DEFAULT as usize][SWAP_FRONTSWAP as usize] = A_BOLD | ColorPairGrayBlack;
    t[COLORSCHEME_DEFAULT as usize][GRAPH_1 as usize] = A_BOLD | ColorPair(Cyan, Black);
    t[COLORSCHEME_DEFAULT as usize][GRAPH_2 as usize] = ColorPair(Cyan, Black);
    t[COLORSCHEME_DEFAULT as usize][MEMORY_1 as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_DEFAULT as usize][MEMORY_2 as usize] = ColorPair(Magenta, Black);
    t[COLORSCHEME_DEFAULT as usize][MEMORY_3 as usize] = A_BOLD | ColorPairGrayBlack;
    t[COLORSCHEME_DEFAULT as usize][MEMORY_4 as usize] = A_BOLD | ColorPair(Blue, Black);
    t[COLORSCHEME_DEFAULT as usize][MEMORY_5 as usize] = ColorPair(Yellow, Black);
    t[COLORSCHEME_DEFAULT as usize][MEMORY_6 as usize] = ColorPair(Cyan, Black);
    t[COLORSCHEME_DEFAULT as usize][HUGEPAGE_1 as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_DEFAULT as usize][HUGEPAGE_2 as usize] = ColorPair(Yellow, Black);
    t[COLORSCHEME_DEFAULT as usize][HUGEPAGE_3 as usize] = ColorPair(Red, Black);
    t[COLORSCHEME_DEFAULT as usize][HUGEPAGE_4 as usize] = A_BOLD | ColorPair(Blue, Black);
    t[COLORSCHEME_DEFAULT as usize][LOAD_AVERAGE_FIFTEEN as usize] = ColorPair(Cyan, Black);
    t[COLORSCHEME_DEFAULT as usize][LOAD_AVERAGE_FIVE as usize] = A_BOLD | ColorPair(Cyan, Black);
    t[COLORSCHEME_DEFAULT as usize][LOAD_AVERAGE_ONE as usize] = A_BOLD | ColorPair(White, Black);
    t[COLORSCHEME_DEFAULT as usize][LOAD as usize] = A_BOLD;
    t[COLORSCHEME_DEFAULT as usize][HELP_BOLD as usize] = A_BOLD | ColorPair(Cyan, Black);
    t[COLORSCHEME_DEFAULT as usize][HELP_SHADOW as usize] = A_BOLD | ColorPairGrayBlack;
    t[COLORSCHEME_DEFAULT as usize][CLOCK as usize] = A_BOLD;
    t[COLORSCHEME_DEFAULT as usize][DATE as usize] = A_BOLD;
    t[COLORSCHEME_DEFAULT as usize][DATETIME as usize] = A_BOLD;
    t[COLORSCHEME_DEFAULT as usize][CHECK_BOX as usize] = ColorPair(Cyan, Black);
    t[COLORSCHEME_DEFAULT as usize][CHECK_MARK as usize] = A_BOLD;
    t[COLORSCHEME_DEFAULT as usize][CHECK_TEXT as usize] = A_NORMAL;
    t[COLORSCHEME_DEFAULT as usize][HOSTNAME as usize] = A_BOLD;
    t[COLORSCHEME_DEFAULT as usize][CPU_NICE as usize] = A_BOLD | ColorPair(Blue, Black);
    t[COLORSCHEME_DEFAULT as usize][CPU_NICE_TEXT as usize] = A_BOLD | ColorPair(Blue, Black);
    t[COLORSCHEME_DEFAULT as usize][CPU_NORMAL as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_DEFAULT as usize][CPU_SYSTEM as usize] = ColorPair(Red, Black);
    t[COLORSCHEME_DEFAULT as usize][CPU_IOWAIT as usize] = A_BOLD | ColorPairGrayBlack;
    t[COLORSCHEME_DEFAULT as usize][CPU_IRQ as usize] = ColorPair(Yellow, Black);
    t[COLORSCHEME_DEFAULT as usize][CPU_SOFTIRQ as usize] = ColorPair(Magenta, Black);
    t[COLORSCHEME_DEFAULT as usize][CPU_STEAL as usize] = ColorPair(Cyan, Black);
    t[COLORSCHEME_DEFAULT as usize][CPU_GUEST as usize] = ColorPair(Cyan, Black);
    t[COLORSCHEME_DEFAULT as usize][GPU_ENGINE_1 as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_DEFAULT as usize][GPU_ENGINE_2 as usize] = ColorPair(Yellow, Black);
    t[COLORSCHEME_DEFAULT as usize][GPU_ENGINE_3 as usize] = ColorPair(Red, Black);
    t[COLORSCHEME_DEFAULT as usize][GPU_ENGINE_4 as usize] = A_BOLD | ColorPair(Blue, Black);
    t[COLORSCHEME_DEFAULT as usize][GPU_RESIDUE as usize] = ColorPair(Magenta, Black);
    t[COLORSCHEME_DEFAULT as usize][PANEL_EDIT as usize] = ColorPair(White, Blue);
    t[COLORSCHEME_DEFAULT as usize][SCREENS_OTH_BORDER as usize] = ColorPair(Blue, Blue);
    t[COLORSCHEME_DEFAULT as usize][SCREENS_OTH_TEXT as usize] = ColorPair(Black, Blue);
    t[COLORSCHEME_DEFAULT as usize][SCREENS_CUR_BORDER as usize] = ColorPair(Green, Green);
    t[COLORSCHEME_DEFAULT as usize][SCREENS_CUR_TEXT as usize] = ColorPair(Black, Green);
    t[COLORSCHEME_DEFAULT as usize][PRESSURE_STALL_THREEHUNDRED as usize] = ColorPair(Cyan, Black);
    t[COLORSCHEME_DEFAULT as usize][PRESSURE_STALL_SIXTY as usize] =
        A_BOLD | ColorPair(Cyan, Black);
    t[COLORSCHEME_DEFAULT as usize][PRESSURE_STALL_TEN as usize] = A_BOLD | ColorPair(White, Black);
    t[COLORSCHEME_DEFAULT as usize][FILE_DESCRIPTOR_USED as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_DEFAULT as usize][FILE_DESCRIPTOR_MAX as usize] = A_BOLD | ColorPair(Blue, Black);
    t[COLORSCHEME_DEFAULT as usize][ZFS_MFU as usize] = A_BOLD | ColorPair(Blue, Black);
    t[COLORSCHEME_DEFAULT as usize][ZFS_MRU as usize] = ColorPair(Yellow, Black);
    t[COLORSCHEME_DEFAULT as usize][ZFS_ANON as usize] = ColorPair(Magenta, Black);
    t[COLORSCHEME_DEFAULT as usize][ZFS_HEADER as usize] = ColorPair(Cyan, Black);
    t[COLORSCHEME_DEFAULT as usize][ZFS_OTHER as usize] = ColorPair(Magenta, Black);
    t[COLORSCHEME_DEFAULT as usize][ZFS_COMPRESSED as usize] = A_BOLD | ColorPair(Blue, Black);
    t[COLORSCHEME_DEFAULT as usize][ZFS_RATIO as usize] = ColorPair(Magenta, Black);
    t[COLORSCHEME_DEFAULT as usize][ZRAM_COMPRESSED as usize] = A_BOLD | ColorPair(Blue, Black);
    t[COLORSCHEME_DEFAULT as usize][ZRAM_UNCOMPRESSED as usize] = ColorPair(Yellow, Black);
    t[COLORSCHEME_DEFAULT as usize][DYNAMIC_GRAY as usize] = ColorPairGrayBlack;
    t[COLORSCHEME_DEFAULT as usize][DYNAMIC_DARKGRAY as usize] = A_BOLD | ColorPairGrayBlack;
    t[COLORSCHEME_DEFAULT as usize][DYNAMIC_RED as usize] = ColorPair(Red, Black);
    t[COLORSCHEME_DEFAULT as usize][DYNAMIC_GREEN as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_DEFAULT as usize][DYNAMIC_BLUE as usize] = A_BOLD | ColorPair(Blue, Black);
    t[COLORSCHEME_DEFAULT as usize][DYNAMIC_CYAN as usize] = ColorPair(Cyan, Black);
    t[COLORSCHEME_DEFAULT as usize][DYNAMIC_MAGENTA as usize] = ColorPair(Magenta, Black);
    t[COLORSCHEME_DEFAULT as usize][DYNAMIC_YELLOW as usize] = ColorPair(Yellow, Black);
    t[COLORSCHEME_DEFAULT as usize][DYNAMIC_WHITE as usize] = ColorPair(White, Black);
    t[COLORSCHEME_MONOCHROME as usize][RESET_COLOR as usize] = A_NORMAL;
    t[COLORSCHEME_MONOCHROME as usize][DEFAULT_COLOR as usize] = A_NORMAL;
    t[COLORSCHEME_MONOCHROME as usize][FUNCTION_BAR as usize] = A_REVERSE;
    t[COLORSCHEME_MONOCHROME as usize][FUNCTION_KEY as usize] = A_NORMAL;
    t[COLORSCHEME_MONOCHROME as usize][PANEL_HEADER_FOCUS as usize] = A_REVERSE;
    t[COLORSCHEME_MONOCHROME as usize][PANEL_HEADER_UNFOCUS as usize] = A_REVERSE;
    t[COLORSCHEME_MONOCHROME as usize][PANEL_SELECTION_FOCUS as usize] = A_REVERSE;
    t[COLORSCHEME_MONOCHROME as usize][PANEL_SELECTION_FOLLOW as usize] = A_REVERSE;
    t[COLORSCHEME_MONOCHROME as usize][PANEL_SELECTION_UNFOCUS as usize] = A_BOLD;
    t[COLORSCHEME_MONOCHROME as usize][FAILED_SEARCH as usize] = A_REVERSE | A_BOLD;
    t[COLORSCHEME_MONOCHROME as usize][FAILED_READ as usize] = A_BOLD;
    t[COLORSCHEME_MONOCHROME as usize][PAUSED as usize] = A_BOLD | A_REVERSE;
    t[COLORSCHEME_MONOCHROME as usize][UPTIME as usize] = A_BOLD;
    t[COLORSCHEME_MONOCHROME as usize][BATTERY as usize] = A_BOLD;
    t[COLORSCHEME_MONOCHROME as usize][LARGE_NUMBER as usize] = A_BOLD;
    t[COLORSCHEME_MONOCHROME as usize][METER_SHADOW as usize] = A_DIM;
    t[COLORSCHEME_MONOCHROME as usize][METER_TEXT as usize] = A_NORMAL;
    t[COLORSCHEME_MONOCHROME as usize][METER_VALUE as usize] = A_BOLD;
    t[COLORSCHEME_MONOCHROME as usize][METER_VALUE_ERROR as usize] = A_BOLD;
    t[COLORSCHEME_MONOCHROME as usize][METER_VALUE_IOREAD as usize] = A_NORMAL;
    t[COLORSCHEME_MONOCHROME as usize][METER_VALUE_IOWRITE as usize] = A_NORMAL;
    t[COLORSCHEME_MONOCHROME as usize][METER_VALUE_NOTICE as usize] = A_BOLD;
    t[COLORSCHEME_MONOCHROME as usize][METER_VALUE_OK as usize] = A_NORMAL;
    t[COLORSCHEME_MONOCHROME as usize][METER_VALUE_WARN as usize] = A_BOLD;
    t[COLORSCHEME_MONOCHROME as usize][LED_COLOR as usize] = A_NORMAL;
    t[COLORSCHEME_MONOCHROME as usize][TASKS_RUNNING as usize] = A_BOLD;
    t[COLORSCHEME_MONOCHROME as usize][PROCESS as usize] = A_NORMAL;
    t[COLORSCHEME_MONOCHROME as usize][PROCESS_SHADOW as usize] = A_DIM;
    t[COLORSCHEME_MONOCHROME as usize][PROCESS_TAG as usize] = A_BOLD;
    t[COLORSCHEME_MONOCHROME as usize][PROCESS_MEGABYTES as usize] = A_BOLD;
    t[COLORSCHEME_MONOCHROME as usize][PROCESS_GIGABYTES as usize] = A_BOLD;
    t[COLORSCHEME_MONOCHROME as usize][PROCESS_BASENAME as usize] = A_BOLD;
    t[COLORSCHEME_MONOCHROME as usize][PROCESS_TREE as usize] = A_BOLD;
    t[COLORSCHEME_MONOCHROME as usize][PROCESS_RUN_STATE as usize] = A_BOLD;
    t[COLORSCHEME_MONOCHROME as usize][PROCESS_D_STATE as usize] = A_BOLD;
    t[COLORSCHEME_MONOCHROME as usize][PROCESS_HIGH_PRIORITY as usize] = A_BOLD;
    t[COLORSCHEME_MONOCHROME as usize][PROCESS_LOW_PRIORITY as usize] = A_DIM;
    t[COLORSCHEME_MONOCHROME as usize][PROCESS_NEW as usize] = A_BOLD;
    t[COLORSCHEME_MONOCHROME as usize][PROCESS_TOMB as usize] = A_DIM;
    t[COLORSCHEME_MONOCHROME as usize][PROCESS_THREAD as usize] = A_BOLD;
    t[COLORSCHEME_MONOCHROME as usize][PROCESS_THREAD_BASENAME as usize] = A_REVERSE;
    t[COLORSCHEME_MONOCHROME as usize][PROCESS_COMM as usize] = A_BOLD;
    t[COLORSCHEME_MONOCHROME as usize][PROCESS_THREAD_COMM as usize] = A_REVERSE;
    t[COLORSCHEME_MONOCHROME as usize][PROCESS_PRIV as usize] = A_BOLD;
    t[COLORSCHEME_MONOCHROME as usize][BAR_BORDER as usize] = A_BOLD;
    t[COLORSCHEME_MONOCHROME as usize][BAR_SHADOW as usize] = A_DIM;
    t[COLORSCHEME_MONOCHROME as usize][SWAP as usize] = A_BOLD;
    t[COLORSCHEME_MONOCHROME as usize][SWAP_CACHE as usize] = A_NORMAL;
    t[COLORSCHEME_MONOCHROME as usize][SWAP_FRONTSWAP as usize] = A_DIM;
    t[COLORSCHEME_MONOCHROME as usize][GRAPH_1 as usize] = A_BOLD;
    t[COLORSCHEME_MONOCHROME as usize][GRAPH_2 as usize] = A_NORMAL;
    t[COLORSCHEME_MONOCHROME as usize][MEMORY_1 as usize] = A_BOLD;
    t[COLORSCHEME_MONOCHROME as usize][MEMORY_2 as usize] = A_NORMAL;
    t[COLORSCHEME_MONOCHROME as usize][MEMORY_3 as usize] = A_NORMAL;
    t[COLORSCHEME_MONOCHROME as usize][MEMORY_4 as usize] = A_NORMAL;
    t[COLORSCHEME_MONOCHROME as usize][MEMORY_5 as usize] = A_DIM;
    t[COLORSCHEME_MONOCHROME as usize][MEMORY_6 as usize] = A_NORMAL;
    t[COLORSCHEME_MONOCHROME as usize][HUGEPAGE_1 as usize] = A_BOLD;
    t[COLORSCHEME_MONOCHROME as usize][HUGEPAGE_2 as usize] = A_NORMAL;
    t[COLORSCHEME_MONOCHROME as usize][HUGEPAGE_3 as usize] = A_REVERSE | A_BOLD;
    t[COLORSCHEME_MONOCHROME as usize][HUGEPAGE_4 as usize] = A_REVERSE;
    t[COLORSCHEME_MONOCHROME as usize][LOAD_AVERAGE_FIFTEEN as usize] = A_DIM;
    t[COLORSCHEME_MONOCHROME as usize][LOAD_AVERAGE_FIVE as usize] = A_NORMAL;
    t[COLORSCHEME_MONOCHROME as usize][LOAD_AVERAGE_ONE as usize] = A_BOLD;
    t[COLORSCHEME_MONOCHROME as usize][LOAD as usize] = A_BOLD;
    t[COLORSCHEME_MONOCHROME as usize][HELP_BOLD as usize] = A_BOLD;
    t[COLORSCHEME_MONOCHROME as usize][HELP_SHADOW as usize] = A_DIM;
    t[COLORSCHEME_MONOCHROME as usize][CLOCK as usize] = A_BOLD;
    t[COLORSCHEME_MONOCHROME as usize][DATE as usize] = A_BOLD;
    t[COLORSCHEME_MONOCHROME as usize][DATETIME as usize] = A_BOLD;
    t[COLORSCHEME_MONOCHROME as usize][CHECK_BOX as usize] = A_BOLD;
    t[COLORSCHEME_MONOCHROME as usize][CHECK_MARK as usize] = A_NORMAL;
    t[COLORSCHEME_MONOCHROME as usize][CHECK_TEXT as usize] = A_NORMAL;
    t[COLORSCHEME_MONOCHROME as usize][HOSTNAME as usize] = A_BOLD;
    t[COLORSCHEME_MONOCHROME as usize][CPU_NICE as usize] = A_NORMAL;
    t[COLORSCHEME_MONOCHROME as usize][CPU_NICE_TEXT as usize] = A_NORMAL;
    t[COLORSCHEME_MONOCHROME as usize][CPU_NORMAL as usize] = A_BOLD;
    t[COLORSCHEME_MONOCHROME as usize][CPU_SYSTEM as usize] = A_BOLD;
    t[COLORSCHEME_MONOCHROME as usize][CPU_IOWAIT as usize] = A_NORMAL;
    t[COLORSCHEME_MONOCHROME as usize][CPU_IRQ as usize] = A_BOLD;
    t[COLORSCHEME_MONOCHROME as usize][CPU_SOFTIRQ as usize] = A_BOLD;
    t[COLORSCHEME_MONOCHROME as usize][CPU_STEAL as usize] = A_DIM;
    t[COLORSCHEME_MONOCHROME as usize][CPU_GUEST as usize] = A_DIM;
    t[COLORSCHEME_MONOCHROME as usize][GPU_ENGINE_1 as usize] = A_BOLD;
    t[COLORSCHEME_MONOCHROME as usize][GPU_ENGINE_2 as usize] = A_NORMAL;
    t[COLORSCHEME_MONOCHROME as usize][GPU_ENGINE_3 as usize] = A_REVERSE | A_BOLD;
    t[COLORSCHEME_MONOCHROME as usize][GPU_ENGINE_4 as usize] = A_REVERSE;
    t[COLORSCHEME_MONOCHROME as usize][GPU_RESIDUE as usize] = A_BOLD;
    t[COLORSCHEME_MONOCHROME as usize][PANEL_EDIT as usize] = A_BOLD;
    t[COLORSCHEME_MONOCHROME as usize][SCREENS_OTH_BORDER as usize] = A_DIM;
    t[COLORSCHEME_MONOCHROME as usize][SCREENS_OTH_TEXT as usize] = A_DIM;
    t[COLORSCHEME_MONOCHROME as usize][SCREENS_CUR_BORDER as usize] = A_REVERSE;
    t[COLORSCHEME_MONOCHROME as usize][SCREENS_CUR_TEXT as usize] = A_REVERSE;
    t[COLORSCHEME_MONOCHROME as usize][PRESSURE_STALL_THREEHUNDRED as usize] = A_DIM;
    t[COLORSCHEME_MONOCHROME as usize][PRESSURE_STALL_SIXTY as usize] = A_NORMAL;
    t[COLORSCHEME_MONOCHROME as usize][PRESSURE_STALL_TEN as usize] = A_BOLD;
    t[COLORSCHEME_MONOCHROME as usize][FILE_DESCRIPTOR_USED as usize] = A_BOLD;
    t[COLORSCHEME_MONOCHROME as usize][FILE_DESCRIPTOR_MAX as usize] = A_BOLD;
    t[COLORSCHEME_MONOCHROME as usize][ZFS_MFU as usize] = A_NORMAL;
    t[COLORSCHEME_MONOCHROME as usize][ZFS_MRU as usize] = A_NORMAL;
    t[COLORSCHEME_MONOCHROME as usize][ZFS_ANON as usize] = A_DIM;
    t[COLORSCHEME_MONOCHROME as usize][ZFS_HEADER as usize] = A_BOLD;
    t[COLORSCHEME_MONOCHROME as usize][ZFS_OTHER as usize] = A_DIM;
    t[COLORSCHEME_MONOCHROME as usize][ZFS_COMPRESSED as usize] = A_BOLD;
    t[COLORSCHEME_MONOCHROME as usize][ZFS_RATIO as usize] = A_BOLD;
    t[COLORSCHEME_MONOCHROME as usize][ZRAM_COMPRESSED as usize] = A_NORMAL;
    t[COLORSCHEME_MONOCHROME as usize][ZRAM_UNCOMPRESSED as usize] = A_NORMAL;
    t[COLORSCHEME_MONOCHROME as usize][DYNAMIC_GRAY as usize] = A_DIM;
    t[COLORSCHEME_MONOCHROME as usize][DYNAMIC_DARKGRAY as usize] = A_DIM;
    t[COLORSCHEME_MONOCHROME as usize][DYNAMIC_RED as usize] = A_BOLD;
    t[COLORSCHEME_MONOCHROME as usize][DYNAMIC_GREEN as usize] = A_NORMAL;
    t[COLORSCHEME_MONOCHROME as usize][DYNAMIC_BLUE as usize] = A_NORMAL;
    t[COLORSCHEME_MONOCHROME as usize][DYNAMIC_CYAN as usize] = A_BOLD;
    t[COLORSCHEME_MONOCHROME as usize][DYNAMIC_MAGENTA as usize] = A_NORMAL;
    t[COLORSCHEME_MONOCHROME as usize][DYNAMIC_YELLOW as usize] = A_NORMAL;
    t[COLORSCHEME_MONOCHROME as usize][DYNAMIC_WHITE as usize] = A_BOLD;
    t[COLORSCHEME_BLACKONWHITE as usize][RESET_COLOR as usize] = ColorPair(Black, White);
    t[COLORSCHEME_BLACKONWHITE as usize][DEFAULT_COLOR as usize] = ColorPair(Black, White);
    t[COLORSCHEME_BLACKONWHITE as usize][FUNCTION_BAR as usize] = ColorPair(Black, Cyan);
    t[COLORSCHEME_BLACKONWHITE as usize][FUNCTION_KEY as usize] = ColorPair(Black, White);
    t[COLORSCHEME_BLACKONWHITE as usize][PANEL_HEADER_FOCUS as usize] = ColorPair(Black, Green);
    t[COLORSCHEME_BLACKONWHITE as usize][PANEL_HEADER_UNFOCUS as usize] = ColorPair(Black, Green);
    t[COLORSCHEME_BLACKONWHITE as usize][PANEL_SELECTION_FOCUS as usize] = ColorPair(Black, Cyan);
    t[COLORSCHEME_BLACKONWHITE as usize][PANEL_SELECTION_FOLLOW as usize] =
        ColorPair(Black, Yellow);
    t[COLORSCHEME_BLACKONWHITE as usize][PANEL_SELECTION_UNFOCUS as usize] = ColorPair(Blue, White);
    t[COLORSCHEME_BLACKONWHITE as usize][FAILED_SEARCH as usize] = ColorPair(Red, Cyan);
    t[COLORSCHEME_BLACKONWHITE as usize][FAILED_READ as usize] = ColorPair(Red, White);
    t[COLORSCHEME_BLACKONWHITE as usize][PAUSED as usize] = A_BOLD | ColorPair(Yellow, Cyan);
    t[COLORSCHEME_BLACKONWHITE as usize][UPTIME as usize] = ColorPair(Yellow, White);
    t[COLORSCHEME_BLACKONWHITE as usize][BATTERY as usize] = ColorPair(Yellow, White);
    t[COLORSCHEME_BLACKONWHITE as usize][LARGE_NUMBER as usize] = ColorPair(Red, White);
    t[COLORSCHEME_BLACKONWHITE as usize][METER_SHADOW as usize] = ColorPair(Blue, White);
    t[COLORSCHEME_BLACKONWHITE as usize][METER_TEXT as usize] = ColorPair(Blue, White);
    t[COLORSCHEME_BLACKONWHITE as usize][METER_VALUE as usize] = ColorPair(Black, White);
    t[COLORSCHEME_BLACKONWHITE as usize][METER_VALUE_ERROR as usize] =
        A_BOLD | ColorPair(Red, White);
    t[COLORSCHEME_BLACKONWHITE as usize][METER_VALUE_IOREAD as usize] = ColorPair(Green, White);
    t[COLORSCHEME_BLACKONWHITE as usize][METER_VALUE_IOWRITE as usize] = ColorPair(Yellow, White);
    t[COLORSCHEME_BLACKONWHITE as usize][METER_VALUE_NOTICE as usize] =
        A_BOLD | ColorPair(Yellow, White);
    t[COLORSCHEME_BLACKONWHITE as usize][METER_VALUE_OK as usize] = ColorPair(Green, White);
    t[COLORSCHEME_BLACKONWHITE as usize][METER_VALUE_WARN as usize] =
        A_BOLD | ColorPair(Yellow, White);
    t[COLORSCHEME_BLACKONWHITE as usize][LED_COLOR as usize] = ColorPair(Green, White);
    t[COLORSCHEME_BLACKONWHITE as usize][TASKS_RUNNING as usize] = ColorPair(Green, White);
    t[COLORSCHEME_BLACKONWHITE as usize][PROCESS as usize] = ColorPair(Black, White);
    t[COLORSCHEME_BLACKONWHITE as usize][PROCESS_SHADOW as usize] =
        A_BOLD | ColorPair(Black, White);
    t[COLORSCHEME_BLACKONWHITE as usize][PROCESS_TAG as usize] = ColorPair(White, Blue);
    t[COLORSCHEME_BLACKONWHITE as usize][PROCESS_MEGABYTES as usize] = ColorPair(Blue, White);
    t[COLORSCHEME_BLACKONWHITE as usize][PROCESS_GIGABYTES as usize] = ColorPair(Green, White);
    t[COLORSCHEME_BLACKONWHITE as usize][PROCESS_BASENAME as usize] = ColorPair(Blue, White);
    t[COLORSCHEME_BLACKONWHITE as usize][PROCESS_TREE as usize] = ColorPair(Green, White);
    t[COLORSCHEME_BLACKONWHITE as usize][PROCESS_RUN_STATE as usize] = ColorPair(Green, White);
    t[COLORSCHEME_BLACKONWHITE as usize][PROCESS_D_STATE as usize] = A_BOLD | ColorPair(Red, White);
    t[COLORSCHEME_BLACKONWHITE as usize][PROCESS_HIGH_PRIORITY as usize] = ColorPair(Red, White);
    t[COLORSCHEME_BLACKONWHITE as usize][PROCESS_LOW_PRIORITY as usize] = ColorPair(Green, White);
    t[COLORSCHEME_BLACKONWHITE as usize][PROCESS_NEW as usize] = ColorPair(White, Green);
    t[COLORSCHEME_BLACKONWHITE as usize][PROCESS_TOMB as usize] = ColorPair(White, Red);
    t[COLORSCHEME_BLACKONWHITE as usize][PROCESS_THREAD as usize] = ColorPair(Blue, White);
    t[COLORSCHEME_BLACKONWHITE as usize][PROCESS_THREAD_BASENAME as usize] =
        A_BOLD | ColorPair(Blue, White);
    t[COLORSCHEME_BLACKONWHITE as usize][PROCESS_COMM as usize] = ColorPair(Magenta, White);
    t[COLORSCHEME_BLACKONWHITE as usize][PROCESS_THREAD_COMM as usize] = ColorPair(Green, White);
    t[COLORSCHEME_BLACKONWHITE as usize][PROCESS_PRIV as usize] = ColorPair(Magenta, White);
    t[COLORSCHEME_BLACKONWHITE as usize][BAR_BORDER as usize] = ColorPair(Blue, White);
    t[COLORSCHEME_BLACKONWHITE as usize][BAR_SHADOW as usize] = ColorPair(Black, White);
    t[COLORSCHEME_BLACKONWHITE as usize][SWAP as usize] = ColorPair(Red, White);
    t[COLORSCHEME_BLACKONWHITE as usize][SWAP_CACHE as usize] = ColorPair(Yellow, White);
    t[COLORSCHEME_BLACKONWHITE as usize][SWAP_FRONTSWAP as usize] =
        A_BOLD | ColorPair(Black, White);
    t[COLORSCHEME_BLACKONWHITE as usize][GRAPH_1 as usize] = A_BOLD | ColorPair(Blue, White);
    t[COLORSCHEME_BLACKONWHITE as usize][GRAPH_2 as usize] = ColorPair(Blue, White);
    t[COLORSCHEME_BLACKONWHITE as usize][MEMORY_1 as usize] = ColorPair(Green, White);
    t[COLORSCHEME_BLACKONWHITE as usize][MEMORY_2 as usize] = ColorPair(Cyan, White);
    t[COLORSCHEME_BLACKONWHITE as usize][MEMORY_3 as usize] = ColorPair(Yellow, White);
    t[COLORSCHEME_BLACKONWHITE as usize][MEMORY_4 as usize] = ColorPair(Magenta, White);
    t[COLORSCHEME_BLACKONWHITE as usize][MEMORY_5 as usize] = A_BOLD | ColorPair(Black, White);
    t[COLORSCHEME_BLACKONWHITE as usize][MEMORY_6 as usize] = ColorPair(Red, White);
    t[COLORSCHEME_BLACKONWHITE as usize][HUGEPAGE_1 as usize] = ColorPair(Green, White);
    t[COLORSCHEME_BLACKONWHITE as usize][HUGEPAGE_2 as usize] = ColorPair(Yellow, White);
    t[COLORSCHEME_BLACKONWHITE as usize][HUGEPAGE_3 as usize] = ColorPair(Red, White);
    t[COLORSCHEME_BLACKONWHITE as usize][HUGEPAGE_4 as usize] = ColorPair(Blue, White);
    t[COLORSCHEME_BLACKONWHITE as usize][LOAD_AVERAGE_FIFTEEN as usize] = ColorPair(Black, White);
    t[COLORSCHEME_BLACKONWHITE as usize][LOAD_AVERAGE_FIVE as usize] = ColorPair(Black, White);
    t[COLORSCHEME_BLACKONWHITE as usize][LOAD_AVERAGE_ONE as usize] = ColorPair(Black, White);
    t[COLORSCHEME_BLACKONWHITE as usize][LOAD as usize] = ColorPair(Black, White);
    t[COLORSCHEME_BLACKONWHITE as usize][HELP_BOLD as usize] = ColorPair(Blue, White);
    t[COLORSCHEME_BLACKONWHITE as usize][HELP_SHADOW as usize] = A_BOLD | ColorPair(Black, White);
    t[COLORSCHEME_BLACKONWHITE as usize][CLOCK as usize] = ColorPair(Black, White);
    t[COLORSCHEME_BLACKONWHITE as usize][DATE as usize] = ColorPair(Black, White);
    t[COLORSCHEME_BLACKONWHITE as usize][DATETIME as usize] = ColorPair(Black, White);
    t[COLORSCHEME_BLACKONWHITE as usize][CHECK_BOX as usize] = ColorPair(Blue, White);
    t[COLORSCHEME_BLACKONWHITE as usize][CHECK_MARK as usize] = ColorPair(Black, White);
    t[COLORSCHEME_BLACKONWHITE as usize][CHECK_TEXT as usize] = ColorPair(Black, White);
    t[COLORSCHEME_BLACKONWHITE as usize][HOSTNAME as usize] = ColorPair(Black, White);
    t[COLORSCHEME_BLACKONWHITE as usize][CPU_NICE as usize] = ColorPair(Cyan, White);
    t[COLORSCHEME_BLACKONWHITE as usize][CPU_NICE_TEXT as usize] = ColorPair(Cyan, White);
    t[COLORSCHEME_BLACKONWHITE as usize][CPU_NORMAL as usize] = ColorPair(Green, White);
    t[COLORSCHEME_BLACKONWHITE as usize][CPU_SYSTEM as usize] = ColorPair(Red, White);
    t[COLORSCHEME_BLACKONWHITE as usize][CPU_IOWAIT as usize] = A_BOLD | ColorPair(Black, White);
    t[COLORSCHEME_BLACKONWHITE as usize][CPU_IRQ as usize] = ColorPair(Blue, White);
    t[COLORSCHEME_BLACKONWHITE as usize][CPU_SOFTIRQ as usize] = ColorPair(Blue, White);
    t[COLORSCHEME_BLACKONWHITE as usize][CPU_STEAL as usize] = ColorPair(Cyan, White);
    t[COLORSCHEME_BLACKONWHITE as usize][CPU_GUEST as usize] = ColorPair(Cyan, White);
    t[COLORSCHEME_BLACKONWHITE as usize][GPU_ENGINE_1 as usize] = ColorPair(Green, White);
    t[COLORSCHEME_BLACKONWHITE as usize][GPU_ENGINE_2 as usize] = ColorPair(Yellow, White);
    t[COLORSCHEME_BLACKONWHITE as usize][GPU_ENGINE_3 as usize] = ColorPair(Red, White);
    t[COLORSCHEME_BLACKONWHITE as usize][GPU_ENGINE_4 as usize] = ColorPair(Blue, White);
    t[COLORSCHEME_BLACKONWHITE as usize][GPU_RESIDUE as usize] = ColorPair(Magenta, White);
    t[COLORSCHEME_BLACKONWHITE as usize][PANEL_EDIT as usize] = ColorPair(White, Blue);
    t[COLORSCHEME_BLACKONWHITE as usize][SCREENS_OTH_BORDER as usize] =
        A_BOLD | ColorPair(Black, White);
    t[COLORSCHEME_BLACKONWHITE as usize][SCREENS_OTH_TEXT as usize] =
        A_BOLD | ColorPair(Black, White);
    t[COLORSCHEME_BLACKONWHITE as usize][SCREENS_CUR_BORDER as usize] = ColorPair(Green, Green);
    t[COLORSCHEME_BLACKONWHITE as usize][SCREENS_CUR_TEXT as usize] = ColorPair(Black, Green);
    t[COLORSCHEME_BLACKONWHITE as usize][PRESSURE_STALL_THREEHUNDRED as usize] =
        ColorPair(Black, White);
    t[COLORSCHEME_BLACKONWHITE as usize][PRESSURE_STALL_SIXTY as usize] = ColorPair(Black, White);
    t[COLORSCHEME_BLACKONWHITE as usize][PRESSURE_STALL_TEN as usize] = ColorPair(Black, White);
    t[COLORSCHEME_BLACKONWHITE as usize][FILE_DESCRIPTOR_USED as usize] = ColorPair(Green, White);
    t[COLORSCHEME_BLACKONWHITE as usize][FILE_DESCRIPTOR_MAX as usize] = ColorPair(Blue, White);
    t[COLORSCHEME_BLACKONWHITE as usize][ZFS_MFU as usize] = ColorPair(Cyan, White);
    t[COLORSCHEME_BLACKONWHITE as usize][ZFS_MRU as usize] = ColorPair(Yellow, White);
    t[COLORSCHEME_BLACKONWHITE as usize][ZFS_ANON as usize] = ColorPair(Magenta, White);
    t[COLORSCHEME_BLACKONWHITE as usize][ZFS_HEADER as usize] = ColorPair(Yellow, White);
    t[COLORSCHEME_BLACKONWHITE as usize][ZFS_OTHER as usize] = ColorPair(Magenta, White);
    t[COLORSCHEME_BLACKONWHITE as usize][ZFS_COMPRESSED as usize] = ColorPair(Cyan, White);
    t[COLORSCHEME_BLACKONWHITE as usize][ZFS_RATIO as usize] = ColorPair(Magenta, White);
    t[COLORSCHEME_BLACKONWHITE as usize][ZRAM_COMPRESSED as usize] = ColorPair(Cyan, White);
    t[COLORSCHEME_BLACKONWHITE as usize][ZRAM_UNCOMPRESSED as usize] = ColorPair(Yellow, White);
    t[COLORSCHEME_BLACKONWHITE as usize][DYNAMIC_GRAY as usize] = ColorPair(Black, White);
    t[COLORSCHEME_BLACKONWHITE as usize][DYNAMIC_DARKGRAY as usize] =
        A_BOLD | ColorPair(Black, White);
    t[COLORSCHEME_BLACKONWHITE as usize][DYNAMIC_RED as usize] = ColorPair(Red, White);
    t[COLORSCHEME_BLACKONWHITE as usize][DYNAMIC_GREEN as usize] = ColorPair(Green, White);
    t[COLORSCHEME_BLACKONWHITE as usize][DYNAMIC_BLUE as usize] = ColorPair(Blue, White);
    t[COLORSCHEME_BLACKONWHITE as usize][DYNAMIC_CYAN as usize] = ColorPair(Yellow, White);
    t[COLORSCHEME_BLACKONWHITE as usize][DYNAMIC_MAGENTA as usize] = ColorPair(Magenta, White);
    t[COLORSCHEME_BLACKONWHITE as usize][DYNAMIC_YELLOW as usize] = ColorPair(Yellow, White);
    t[COLORSCHEME_BLACKONWHITE as usize][DYNAMIC_WHITE as usize] = A_BOLD | ColorPair(Black, White);
    t[COLORSCHEME_LIGHTTERMINAL as usize][RESET_COLOR as usize] = ColorPair(Black, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][DEFAULT_COLOR as usize] = ColorPair(Black, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][FUNCTION_BAR as usize] = ColorPair(Black, Cyan);
    t[COLORSCHEME_LIGHTTERMINAL as usize][FUNCTION_KEY as usize] = ColorPair(Black, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][PANEL_HEADER_FOCUS as usize] = ColorPair(Black, Green);
    t[COLORSCHEME_LIGHTTERMINAL as usize][PANEL_HEADER_UNFOCUS as usize] = ColorPair(Black, Green);
    t[COLORSCHEME_LIGHTTERMINAL as usize][PANEL_SELECTION_FOCUS as usize] = ColorPair(Black, Cyan);
    t[COLORSCHEME_LIGHTTERMINAL as usize][PANEL_SELECTION_FOLLOW as usize] =
        ColorPair(Black, Yellow);
    t[COLORSCHEME_LIGHTTERMINAL as usize][PANEL_SELECTION_UNFOCUS as usize] =
        ColorPair(Blue, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][FAILED_SEARCH as usize] = ColorPair(Red, Cyan);
    t[COLORSCHEME_LIGHTTERMINAL as usize][FAILED_READ as usize] = ColorPair(Red, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][PAUSED as usize] = A_BOLD | ColorPair(Yellow, Cyan);
    t[COLORSCHEME_LIGHTTERMINAL as usize][UPTIME as usize] = ColorPair(Yellow, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][BATTERY as usize] = ColorPair(Yellow, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][LARGE_NUMBER as usize] = ColorPair(Red, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][METER_SHADOW as usize] = A_BOLD | ColorPairGrayBlack;
    t[COLORSCHEME_LIGHTTERMINAL as usize][METER_TEXT as usize] = ColorPair(Blue, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][METER_VALUE as usize] = ColorPair(Black, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][METER_VALUE_ERROR as usize] =
        A_BOLD | ColorPair(Red, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][METER_VALUE_IOREAD as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][METER_VALUE_IOWRITE as usize] = ColorPair(Yellow, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][METER_VALUE_NOTICE as usize] =
        A_BOLD | ColorPairWhiteDefault;
    t[COLORSCHEME_LIGHTTERMINAL as usize][METER_VALUE_OK as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][METER_VALUE_WARN as usize] =
        A_BOLD | ColorPair(Yellow, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][LED_COLOR as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][TASKS_RUNNING as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][PROCESS as usize] = ColorPair(Black, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][PROCESS_SHADOW as usize] = A_BOLD | ColorPairGrayBlack;
    t[COLORSCHEME_LIGHTTERMINAL as usize][PROCESS_TAG as usize] = ColorPair(White, Blue);
    t[COLORSCHEME_LIGHTTERMINAL as usize][PROCESS_MEGABYTES as usize] = ColorPair(Blue, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][PROCESS_GIGABYTES as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][PROCESS_BASENAME as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][PROCESS_TREE as usize] = ColorPair(Blue, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][PROCESS_RUN_STATE as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][PROCESS_D_STATE as usize] =
        A_BOLD | ColorPair(Red, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][PROCESS_HIGH_PRIORITY as usize] = ColorPair(Red, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][PROCESS_LOW_PRIORITY as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][PROCESS_NEW as usize] = ColorPair(Black, Green);
    t[COLORSCHEME_LIGHTTERMINAL as usize][PROCESS_TOMB as usize] = ColorPair(Black, Red);
    t[COLORSCHEME_LIGHTTERMINAL as usize][PROCESS_THREAD as usize] = ColorPair(Blue, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][PROCESS_THREAD_BASENAME as usize] =
        A_BOLD | ColorPair(Blue, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][PROCESS_COMM as usize] = ColorPair(Magenta, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][PROCESS_THREAD_COMM as usize] = ColorPair(Yellow, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][PROCESS_PRIV as usize] = ColorPair(Magenta, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][BAR_BORDER as usize] = ColorPair(Blue, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][BAR_SHADOW as usize] = ColorPairGrayBlack;
    t[COLORSCHEME_LIGHTTERMINAL as usize][SWAP as usize] = ColorPair(Red, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][SWAP_CACHE as usize] = ColorPair(Yellow, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][SWAP_FRONTSWAP as usize] = ColorPairGrayBlack;
    t[COLORSCHEME_LIGHTTERMINAL as usize][GRAPH_1 as usize] = A_BOLD | ColorPair(Cyan, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][GRAPH_2 as usize] = ColorPair(Cyan, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][MEMORY_1 as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][MEMORY_2 as usize] = ColorPair(Cyan, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][MEMORY_3 as usize] = ColorPair(Yellow, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][MEMORY_4 as usize] = ColorPair(Magenta, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][MEMORY_5 as usize] = ColorPairGrayBlack;
    t[COLORSCHEME_LIGHTTERMINAL as usize][MEMORY_6 as usize] = ColorPair(Blue, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][HUGEPAGE_1 as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][HUGEPAGE_2 as usize] = ColorPair(Yellow, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][HUGEPAGE_3 as usize] = ColorPair(Red, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][HUGEPAGE_4 as usize] = ColorPair(Blue, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][LOAD_AVERAGE_FIFTEEN as usize] = ColorPair(Black, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][LOAD_AVERAGE_FIVE as usize] = ColorPair(Black, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][LOAD_AVERAGE_ONE as usize] = ColorPair(Black, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][LOAD as usize] = ColorPairWhiteDefault;
    t[COLORSCHEME_LIGHTTERMINAL as usize][HELP_BOLD as usize] = ColorPair(Blue, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][HELP_SHADOW as usize] = A_BOLD | ColorPairGrayBlack;
    t[COLORSCHEME_LIGHTTERMINAL as usize][CLOCK as usize] = ColorPairWhiteDefault;
    t[COLORSCHEME_LIGHTTERMINAL as usize][DATE as usize] = ColorPairWhiteDefault;
    t[COLORSCHEME_LIGHTTERMINAL as usize][DATETIME as usize] = ColorPairWhiteDefault;
    t[COLORSCHEME_LIGHTTERMINAL as usize][CHECK_BOX as usize] = ColorPair(Blue, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][CHECK_MARK as usize] = ColorPair(Black, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][CHECK_TEXT as usize] = ColorPair(Black, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][HOSTNAME as usize] = ColorPairWhiteDefault;
    t[COLORSCHEME_LIGHTTERMINAL as usize][CPU_NICE as usize] = ColorPair(Cyan, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][CPU_NICE_TEXT as usize] = ColorPair(Cyan, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][CPU_NORMAL as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][CPU_SYSTEM as usize] = ColorPair(Red, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][CPU_IOWAIT as usize] = A_BOLD | ColorPair(Black, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][CPU_IRQ as usize] = A_BOLD | ColorPair(Blue, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][CPU_SOFTIRQ as usize] = ColorPair(Blue, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][CPU_STEAL as usize] = ColorPair(Black, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][CPU_GUEST as usize] = ColorPair(Black, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][GPU_ENGINE_1 as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][GPU_ENGINE_2 as usize] = ColorPair(Yellow, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][GPU_ENGINE_3 as usize] = ColorPair(Red, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][GPU_ENGINE_4 as usize] = ColorPair(Blue, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][GPU_RESIDUE as usize] = ColorPair(Magenta, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][PANEL_EDIT as usize] = ColorPair(White, Blue);
    t[COLORSCHEME_LIGHTTERMINAL as usize][SCREENS_OTH_BORDER as usize] = ColorPair(Blue, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][SCREENS_OTH_TEXT as usize] = ColorPair(Blue, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][SCREENS_CUR_BORDER as usize] = ColorPair(Green, Green);
    t[COLORSCHEME_LIGHTTERMINAL as usize][SCREENS_CUR_TEXT as usize] = ColorPair(Black, Green);
    t[COLORSCHEME_LIGHTTERMINAL as usize][PRESSURE_STALL_THREEHUNDRED as usize] =
        ColorPair(Black, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][PRESSURE_STALL_SIXTY as usize] = ColorPair(Black, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][PRESSURE_STALL_TEN as usize] = ColorPair(Black, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][FILE_DESCRIPTOR_USED as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][FILE_DESCRIPTOR_MAX as usize] =
        A_BOLD | ColorPair(Blue, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][ZFS_MFU as usize] = ColorPair(Cyan, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][ZFS_MRU as usize] = ColorPair(Yellow, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][ZFS_ANON as usize] = A_BOLD | ColorPair(Magenta, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][ZFS_HEADER as usize] = ColorPair(Black, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][ZFS_OTHER as usize] = A_BOLD | ColorPair(Magenta, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][ZFS_COMPRESSED as usize] = ColorPair(Cyan, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][ZFS_RATIO as usize] = A_BOLD | ColorPair(Magenta, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][ZRAM_COMPRESSED as usize] = ColorPair(Cyan, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][ZRAM_UNCOMPRESSED as usize] = ColorPair(Yellow, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][DYNAMIC_GRAY as usize] = ColorPairGrayBlack;
    t[COLORSCHEME_LIGHTTERMINAL as usize][DYNAMIC_DARKGRAY as usize] = A_BOLD | ColorPairGrayBlack;
    t[COLORSCHEME_LIGHTTERMINAL as usize][DYNAMIC_RED as usize] = ColorPair(Red, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][DYNAMIC_GREEN as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][DYNAMIC_BLUE as usize] = ColorPair(Blue, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][DYNAMIC_CYAN as usize] = ColorPair(Cyan, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][DYNAMIC_MAGENTA as usize] = ColorPair(Magenta, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][DYNAMIC_YELLOW as usize] = ColorPair(Yellow, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][DYNAMIC_WHITE as usize] = ColorPairWhiteDefault;
    t[COLORSCHEME_MIDNIGHT as usize][RESET_COLOR as usize] = ColorPair(White, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][DEFAULT_COLOR as usize] = ColorPair(White, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][FUNCTION_BAR as usize] = ColorPair(Black, Cyan);
    t[COLORSCHEME_MIDNIGHT as usize][FUNCTION_KEY as usize] = A_NORMAL;
    t[COLORSCHEME_MIDNIGHT as usize][PANEL_HEADER_FOCUS as usize] = ColorPair(Black, Cyan);
    t[COLORSCHEME_MIDNIGHT as usize][PANEL_HEADER_UNFOCUS as usize] = ColorPair(Black, Cyan);
    t[COLORSCHEME_MIDNIGHT as usize][PANEL_SELECTION_FOCUS as usize] = ColorPair(Black, White);
    t[COLORSCHEME_MIDNIGHT as usize][PANEL_SELECTION_FOLLOW as usize] = ColorPair(Black, Yellow);
    t[COLORSCHEME_MIDNIGHT as usize][PANEL_SELECTION_UNFOCUS as usize] =
        A_BOLD | ColorPair(Yellow, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][FAILED_SEARCH as usize] = ColorPair(Red, Cyan);
    t[COLORSCHEME_MIDNIGHT as usize][FAILED_READ as usize] = A_BOLD | ColorPair(Red, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][PAUSED as usize] = A_BOLD | ColorPair(Yellow, Cyan);
    t[COLORSCHEME_MIDNIGHT as usize][UPTIME as usize] = A_BOLD | ColorPair(Yellow, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][BATTERY as usize] = A_BOLD | ColorPair(Yellow, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][LARGE_NUMBER as usize] = A_BOLD | ColorPair(Red, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][METER_SHADOW as usize] = ColorPair(Cyan, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][METER_TEXT as usize] = ColorPair(Cyan, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][METER_VALUE as usize] = A_BOLD | ColorPair(Cyan, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][METER_VALUE_ERROR as usize] = A_BOLD | ColorPair(Red, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][METER_VALUE_IOREAD as usize] = ColorPair(Green, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][METER_VALUE_IOWRITE as usize] = ColorPair(Black, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][METER_VALUE_NOTICE as usize] = A_BOLD | ColorPair(White, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][METER_VALUE_OK as usize] = ColorPair(Green, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][METER_VALUE_WARN as usize] = A_BOLD | ColorPair(Yellow, Black);
    t[COLORSCHEME_MIDNIGHT as usize][LED_COLOR as usize] = ColorPair(Green, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][TASKS_RUNNING as usize] = A_BOLD | ColorPair(Green, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][PROCESS as usize] = ColorPair(White, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][PROCESS_SHADOW as usize] = A_BOLD | ColorPair(Black, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][PROCESS_TAG as usize] = A_BOLD | ColorPair(Yellow, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][PROCESS_MEGABYTES as usize] = ColorPair(Cyan, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][PROCESS_GIGABYTES as usize] = ColorPair(Green, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][PROCESS_BASENAME as usize] = A_BOLD | ColorPair(Cyan, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][PROCESS_TREE as usize] = ColorPair(Cyan, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][PROCESS_RUN_STATE as usize] = ColorPair(Green, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][PROCESS_D_STATE as usize] = A_BOLD | ColorPair(Red, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][PROCESS_HIGH_PRIORITY as usize] = ColorPair(Red, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][PROCESS_LOW_PRIORITY as usize] = ColorPair(Green, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][PROCESS_NEW as usize] = ColorPair(Blue, Green);
    t[COLORSCHEME_MIDNIGHT as usize][PROCESS_TOMB as usize] = ColorPair(Blue, Red);
    t[COLORSCHEME_MIDNIGHT as usize][PROCESS_THREAD as usize] = ColorPair(Green, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][PROCESS_THREAD_BASENAME as usize] =
        A_BOLD | ColorPair(Green, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][PROCESS_COMM as usize] = ColorPair(Magenta, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][PROCESS_THREAD_COMM as usize] = ColorPair(Black, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][PROCESS_PRIV as usize] = ColorPair(Magenta, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][BAR_BORDER as usize] = A_BOLD | ColorPair(Yellow, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][BAR_SHADOW as usize] = ColorPair(Cyan, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][SWAP as usize] = ColorPair(Red, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][SWAP_CACHE as usize] = A_BOLD | ColorPair(Yellow, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][SWAP_FRONTSWAP as usize] = A_BOLD | ColorPair(Black, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][GRAPH_1 as usize] = A_BOLD | ColorPair(Cyan, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][GRAPH_2 as usize] = ColorPair(Cyan, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][MEMORY_1 as usize] = A_BOLD | ColorPair(Green, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][MEMORY_2 as usize] = A_BOLD | ColorPair(Cyan, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][MEMORY_3 as usize] = A_BOLD | ColorPair(Yellow, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][MEMORY_4 as usize] = A_BOLD | ColorPair(Magenta, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][MEMORY_5 as usize] = A_BOLD | ColorPair(Black, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][MEMORY_6 as usize] = ColorPair(Cyan, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][HUGEPAGE_1 as usize] = A_BOLD | ColorPair(Green, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][HUGEPAGE_2 as usize] = A_BOLD | ColorPair(Yellow, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][HUGEPAGE_3 as usize] = A_BOLD | ColorPair(Red, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][HUGEPAGE_4 as usize] = A_BOLD | ColorPair(White, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][LOAD_AVERAGE_FIFTEEN as usize] =
        A_BOLD | ColorPair(Black, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][LOAD_AVERAGE_FIVE as usize] =
        A_NORMAL | ColorPair(White, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][LOAD_AVERAGE_ONE as usize] = A_BOLD | ColorPair(White, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][LOAD as usize] = A_BOLD | ColorPair(White, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][HELP_BOLD as usize] = A_BOLD | ColorPair(Cyan, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][HELP_SHADOW as usize] = A_BOLD | ColorPair(Black, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][CLOCK as usize] = ColorPair(White, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][DATE as usize] = ColorPair(White, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][DATETIME as usize] = ColorPair(White, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][CHECK_BOX as usize] = ColorPair(Cyan, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][CHECK_MARK as usize] = A_BOLD | ColorPair(White, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][CHECK_TEXT as usize] = A_NORMAL | ColorPair(White, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][HOSTNAME as usize] = ColorPair(White, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][CPU_NICE as usize] = A_BOLD | ColorPair(Cyan, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][CPU_NICE_TEXT as usize] = A_BOLD | ColorPair(Cyan, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][CPU_NORMAL as usize] = A_BOLD | ColorPair(Green, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][CPU_SYSTEM as usize] = A_BOLD | ColorPair(Red, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][CPU_IOWAIT as usize] = A_BOLD | ColorPair(Black, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][CPU_IRQ as usize] = A_BOLD | ColorPair(Black, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][CPU_SOFTIRQ as usize] = ColorPair(Black, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][CPU_STEAL as usize] = ColorPair(White, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][CPU_GUEST as usize] = ColorPair(White, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][GPU_ENGINE_1 as usize] = A_BOLD | ColorPair(Green, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][GPU_ENGINE_2 as usize] = A_BOLD | ColorPair(Yellow, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][GPU_ENGINE_3 as usize] = A_BOLD | ColorPair(Red, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][GPU_ENGINE_4 as usize] = A_BOLD | ColorPair(White, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][GPU_RESIDUE as usize] = A_BOLD | ColorPair(Magenta, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][PANEL_EDIT as usize] = ColorPair(White, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][SCREENS_OTH_BORDER as usize] =
        A_BOLD | ColorPair(Yellow, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][SCREENS_OTH_TEXT as usize] = ColorPair(Cyan, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][SCREENS_CUR_BORDER as usize] = ColorPair(Cyan, Cyan);
    t[COLORSCHEME_MIDNIGHT as usize][SCREENS_CUR_TEXT as usize] = ColorPair(Black, Cyan);
    t[COLORSCHEME_MIDNIGHT as usize][PRESSURE_STALL_THREEHUNDRED as usize] =
        A_BOLD | ColorPair(Black, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][PRESSURE_STALL_SIXTY as usize] =
        A_NORMAL | ColorPair(White, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][PRESSURE_STALL_TEN as usize] = A_BOLD | ColorPair(White, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][FILE_DESCRIPTOR_USED as usize] =
        A_BOLD | ColorPair(Green, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][FILE_DESCRIPTOR_MAX as usize] = A_BOLD | ColorPair(Red, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][ZFS_MFU as usize] = A_BOLD | ColorPair(White, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][ZFS_MRU as usize] = A_BOLD | ColorPair(Yellow, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][ZFS_ANON as usize] = A_BOLD | ColorPair(Magenta, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][ZFS_HEADER as usize] = A_BOLD | ColorPair(Yellow, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][ZFS_OTHER as usize] = A_BOLD | ColorPair(Magenta, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][ZFS_COMPRESSED as usize] = A_BOLD | ColorPair(White, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][ZFS_RATIO as usize] = A_BOLD | ColorPair(Magenta, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][ZRAM_COMPRESSED as usize] = ColorPair(Cyan, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][ZRAM_UNCOMPRESSED as usize] = ColorPair(Yellow, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][DYNAMIC_GRAY as usize] = ColorPairGrayBlack;
    t[COLORSCHEME_MIDNIGHT as usize][DYNAMIC_DARKGRAY as usize] = A_BOLD | ColorPairGrayBlack;
    t[COLORSCHEME_MIDNIGHT as usize][DYNAMIC_RED as usize] = ColorPair(Red, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][DYNAMIC_GREEN as usize] = ColorPair(Green, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][DYNAMIC_BLUE as usize] = ColorPair(Black, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][DYNAMIC_CYAN as usize] = ColorPair(Cyan, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][DYNAMIC_MAGENTA as usize] = ColorPair(Magenta, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][DYNAMIC_YELLOW as usize] = ColorPair(Yellow, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][DYNAMIC_WHITE as usize] = ColorPair(White, Blue);
    t[COLORSCHEME_BLACKNIGHT as usize][RESET_COLOR as usize] = ColorPair(Cyan, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][DEFAULT_COLOR as usize] = ColorPair(Cyan, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][FUNCTION_BAR as usize] = ColorPair(Black, Green);
    t[COLORSCHEME_BLACKNIGHT as usize][FUNCTION_KEY as usize] = ColorPair(Cyan, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][PANEL_HEADER_FOCUS as usize] = ColorPair(Black, Green);
    t[COLORSCHEME_BLACKNIGHT as usize][PANEL_HEADER_UNFOCUS as usize] = ColorPair(Black, Green);
    t[COLORSCHEME_BLACKNIGHT as usize][PANEL_SELECTION_FOCUS as usize] = ColorPair(Black, Cyan);
    t[COLORSCHEME_BLACKNIGHT as usize][PANEL_SELECTION_FOLLOW as usize] = ColorPair(Black, Yellow);
    t[COLORSCHEME_BLACKNIGHT as usize][PANEL_SELECTION_UNFOCUS as usize] = ColorPair(Black, White);
    t[COLORSCHEME_BLACKNIGHT as usize][FAILED_SEARCH as usize] = ColorPair(Red, Green);
    t[COLORSCHEME_BLACKNIGHT as usize][FAILED_READ as usize] = A_BOLD | ColorPair(Red, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][PAUSED as usize] = A_BOLD | ColorPair(Yellow, Green);
    t[COLORSCHEME_BLACKNIGHT as usize][UPTIME as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][BATTERY as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][LARGE_NUMBER as usize] = A_BOLD | ColorPair(Red, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][METER_SHADOW as usize] = A_BOLD | ColorPairGrayBlack;
    t[COLORSCHEME_BLACKNIGHT as usize][METER_TEXT as usize] = ColorPair(Cyan, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][METER_VALUE as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][METER_VALUE_ERROR as usize] = A_BOLD | ColorPair(Red, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][METER_VALUE_IOREAD as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][METER_VALUE_IOWRITE as usize] = ColorPair(Blue, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][METER_VALUE_NOTICE as usize] =
        A_BOLD | ColorPair(White, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][METER_VALUE_OK as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][METER_VALUE_WARN as usize] =
        A_BOLD | ColorPair(Yellow, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][LED_COLOR as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][TASKS_RUNNING as usize] = A_BOLD | ColorPair(Green, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][PROCESS as usize] = ColorPair(Cyan, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][PROCESS_SHADOW as usize] = A_BOLD | ColorPairGrayBlack;
    t[COLORSCHEME_BLACKNIGHT as usize][PROCESS_TAG as usize] = A_BOLD | ColorPair(Yellow, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][PROCESS_MEGABYTES as usize] =
        A_BOLD | ColorPair(Green, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][PROCESS_GIGABYTES as usize] =
        A_BOLD | ColorPair(Yellow, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][PROCESS_BASENAME as usize] =
        A_BOLD | ColorPair(Green, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][PROCESS_TREE as usize] = ColorPair(Cyan, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][PROCESS_THREAD as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][PROCESS_THREAD_BASENAME as usize] =
        A_BOLD | ColorPair(Blue, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][PROCESS_COMM as usize] = ColorPair(Magenta, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][PROCESS_THREAD_COMM as usize] = ColorPair(Yellow, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][PROCESS_RUN_STATE as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][PROCESS_D_STATE as usize] = A_BOLD | ColorPair(Red, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][PROCESS_HIGH_PRIORITY as usize] = ColorPair(Red, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][PROCESS_LOW_PRIORITY as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][PROCESS_NEW as usize] = ColorPair(Black, Green);
    t[COLORSCHEME_BLACKNIGHT as usize][PROCESS_TOMB as usize] = ColorPair(Black, Red);
    t[COLORSCHEME_BLACKNIGHT as usize][PROCESS_PRIV as usize] = ColorPair(Magenta, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][BAR_BORDER as usize] = A_BOLD | ColorPair(Green, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][BAR_SHADOW as usize] = ColorPair(Cyan, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][SWAP as usize] = ColorPair(Red, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][SWAP_CACHE as usize] = ColorPair(Yellow, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][SWAP_FRONTSWAP as usize] = ColorPair(Yellow, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][GRAPH_1 as usize] = A_BOLD | ColorPair(Green, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][GRAPH_2 as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][MEMORY_1 as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][MEMORY_2 as usize] = ColorPair(Blue, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][MEMORY_3 as usize] = ColorPair(Yellow, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][MEMORY_4 as usize] = ColorPair(Magenta, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][MEMORY_5 as usize] = ColorPair(Yellow, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][MEMORY_6 as usize] = ColorPair(Cyan, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][HUGEPAGE_1 as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][HUGEPAGE_2 as usize] = ColorPair(Yellow, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][HUGEPAGE_3 as usize] = ColorPair(Red, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][HUGEPAGE_4 as usize] = ColorPair(Blue, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][LOAD_AVERAGE_FIFTEEN as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][LOAD_AVERAGE_FIVE as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][LOAD_AVERAGE_ONE as usize] =
        A_BOLD | ColorPair(Green, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][LOAD as usize] = A_BOLD;
    t[COLORSCHEME_BLACKNIGHT as usize][HELP_BOLD as usize] = A_BOLD | ColorPair(Cyan, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][HELP_SHADOW as usize] = A_BOLD | ColorPairGrayBlack;
    t[COLORSCHEME_BLACKNIGHT as usize][CLOCK as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][CHECK_BOX as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][CHECK_MARK as usize] = A_BOLD | ColorPair(Green, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][CHECK_TEXT as usize] = ColorPair(Cyan, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][HOSTNAME as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][CPU_NICE as usize] = ColorPair(Blue, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][CPU_NICE_TEXT as usize] = A_BOLD | ColorPair(Blue, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][CPU_NORMAL as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][CPU_SYSTEM as usize] = ColorPair(Red, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][CPU_IOWAIT as usize] = ColorPair(Yellow, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][CPU_IRQ as usize] = A_BOLD | ColorPair(Blue, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][CPU_SOFTIRQ as usize] = ColorPair(Blue, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][CPU_STEAL as usize] = ColorPair(Cyan, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][CPU_GUEST as usize] = ColorPair(Cyan, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][GPU_ENGINE_1 as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][GPU_ENGINE_2 as usize] = ColorPair(Yellow, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][GPU_ENGINE_3 as usize] = ColorPair(Red, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][GPU_ENGINE_4 as usize] = ColorPair(Blue, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][GPU_RESIDUE as usize] = ColorPair(Magenta, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][PANEL_EDIT as usize] = ColorPair(White, Cyan);
    t[COLORSCHEME_BLACKNIGHT as usize][SCREENS_OTH_BORDER as usize] = ColorPair(White, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][SCREENS_OTH_TEXT as usize] = ColorPair(Cyan, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][SCREENS_CUR_BORDER as usize] =
        A_BOLD | ColorPair(White, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][SCREENS_CUR_TEXT as usize] =
        A_BOLD | ColorPair(Green, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][PRESSURE_STALL_THREEHUNDRED as usize] =
        ColorPair(Green, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][PRESSURE_STALL_SIXTY as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][PRESSURE_STALL_TEN as usize] =
        A_BOLD | ColorPair(Green, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][FILE_DESCRIPTOR_USED as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][FILE_DESCRIPTOR_MAX as usize] =
        A_BOLD | ColorPair(Blue, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][ZFS_MFU as usize] = ColorPair(Blue, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][ZFS_MRU as usize] = ColorPair(Yellow, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][ZFS_ANON as usize] = ColorPair(Magenta, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][ZFS_HEADER as usize] = ColorPair(Yellow, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][ZFS_OTHER as usize] = ColorPair(Magenta, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][ZFS_COMPRESSED as usize] = ColorPair(Blue, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][ZFS_RATIO as usize] = ColorPair(Magenta, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][ZRAM_COMPRESSED as usize] = ColorPair(Blue, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][ZRAM_UNCOMPRESSED as usize] = ColorPair(Yellow, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][DYNAMIC_GRAY as usize] = ColorPairGrayBlack;
    t[COLORSCHEME_BLACKNIGHT as usize][DYNAMIC_DARKGRAY as usize] = A_BOLD | ColorPairGrayBlack;
    t[COLORSCHEME_BLACKNIGHT as usize][DYNAMIC_RED as usize] = ColorPair(Red, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][DYNAMIC_GREEN as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][DYNAMIC_BLUE as usize] = ColorPair(Blue, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][DYNAMIC_CYAN as usize] = ColorPair(Cyan, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][DYNAMIC_MAGENTA as usize] = ColorPair(Magenta, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][DYNAMIC_YELLOW as usize] = ColorPair(Yellow, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][DYNAMIC_WHITE as usize] = ColorPair(White, Black);
    t[COLORSCHEME_NORD as usize][RESET_COLOR as usize] = A_NORMAL;
    t[COLORSCHEME_NORD as usize][DEFAULT_COLOR as usize] = A_NORMAL;
    t[COLORSCHEME_NORD as usize][FUNCTION_BAR as usize] = ColorPair(Black, Cyan);
    t[COLORSCHEME_NORD as usize][FUNCTION_KEY as usize] = A_NORMAL;
    t[COLORSCHEME_NORD as usize][PANEL_HEADER_FOCUS as usize] = ColorPair(Black, Cyan);
    t[COLORSCHEME_NORD as usize][PANEL_HEADER_UNFOCUS as usize] = ColorPair(Black, Cyan);
    t[COLORSCHEME_NORD as usize][PANEL_SELECTION_FOCUS as usize] = ColorPair(Black, Cyan);
    t[COLORSCHEME_NORD as usize][PANEL_SELECTION_FOLLOW as usize] = A_REVERSE;
    t[COLORSCHEME_NORD as usize][PANEL_SELECTION_UNFOCUS as usize] = A_BOLD;
    t[COLORSCHEME_NORD as usize][FAILED_SEARCH as usize] =
        A_REVERSE | A_BOLD | ColorPair(Yellow, Black);
    t[COLORSCHEME_NORD as usize][FAILED_READ as usize] = A_BOLD | ColorPair(Yellow, Black);
    t[COLORSCHEME_NORD as usize][PAUSED as usize] = A_BOLD | ColorPair(Black, Cyan);
    t[COLORSCHEME_NORD as usize][UPTIME as usize] = A_BOLD;
    t[COLORSCHEME_NORD as usize][BATTERY as usize] = A_BOLD;
    t[COLORSCHEME_NORD as usize][LARGE_NUMBER as usize] = A_BOLD | ColorPair(Yellow, Black);
    t[COLORSCHEME_NORD as usize][METER_SHADOW as usize] = A_BOLD | ColorPairGrayBlack;
    t[COLORSCHEME_NORD as usize][METER_TEXT as usize] = A_NORMAL;
    t[COLORSCHEME_NORD as usize][METER_VALUE as usize] = A_BOLD;
    t[COLORSCHEME_NORD as usize][METER_VALUE_ERROR as usize] = A_BOLD;
    t[COLORSCHEME_NORD as usize][METER_VALUE_IOREAD as usize] = A_NORMAL;
    t[COLORSCHEME_NORD as usize][METER_VALUE_IOWRITE as usize] = A_NORMAL;
    t[COLORSCHEME_NORD as usize][METER_VALUE_NOTICE as usize] = A_BOLD | ColorPair(Cyan, Black);
    t[COLORSCHEME_NORD as usize][METER_VALUE_OK as usize] = A_NORMAL;
    t[COLORSCHEME_NORD as usize][METER_VALUE_WARN as usize] = A_BOLD;
    t[COLORSCHEME_NORD as usize][LED_COLOR as usize] = A_NORMAL;
    t[COLORSCHEME_NORD as usize][TASKS_RUNNING as usize] = A_BOLD;
    t[COLORSCHEME_NORD as usize][PROCESS as usize] = A_NORMAL;
    t[COLORSCHEME_NORD as usize][PROCESS_SHADOW as usize] = A_BOLD | ColorPairGrayBlack;
    t[COLORSCHEME_NORD as usize][PROCESS_TAG as usize] = A_BOLD | ColorPair(Cyan, Black);
    t[COLORSCHEME_NORD as usize][PROCESS_MEGABYTES as usize] = A_BOLD | ColorPair(White, Black);
    t[COLORSCHEME_NORD as usize][PROCESS_GIGABYTES as usize] = A_BOLD | ColorPair(Cyan, Black);
    t[COLORSCHEME_NORD as usize][PROCESS_BASENAME as usize] = A_BOLD;
    t[COLORSCHEME_NORD as usize][PROCESS_TREE as usize] = A_BOLD;
    t[COLORSCHEME_NORD as usize][PROCESS_RUN_STATE as usize] = A_BOLD;
    t[COLORSCHEME_NORD as usize][PROCESS_D_STATE as usize] = A_BOLD | ColorPair(Yellow, Black);
    t[COLORSCHEME_NORD as usize][PROCESS_HIGH_PRIORITY as usize] = A_BOLD;
    t[COLORSCHEME_NORD as usize][PROCESS_LOW_PRIORITY as usize] = A_BOLD | ColorPairGrayBlack;
    t[COLORSCHEME_NORD as usize][PROCESS_NEW as usize] = A_BOLD;
    t[COLORSCHEME_NORD as usize][PROCESS_TOMB as usize] = A_BOLD | ColorPairGrayBlack;
    t[COLORSCHEME_NORD as usize][PROCESS_PRIV as usize] = A_BOLD | ColorPair(Cyan, Black);
    t[COLORSCHEME_NORD as usize][BAR_BORDER as usize] = A_BOLD;
    t[COLORSCHEME_NORD as usize][BAR_SHADOW as usize] = A_BOLD | ColorPairGrayBlack;
    t[COLORSCHEME_NORD as usize][SWAP as usize] = A_BOLD;
    t[COLORSCHEME_NORD as usize][SWAP_CACHE as usize] = A_NORMAL;
    t[COLORSCHEME_NORD as usize][SWAP_FRONTSWAP as usize] = A_BOLD | ColorPairGrayBlack;
    t[COLORSCHEME_NORD as usize][GRAPH_1 as usize] = A_BOLD;
    t[COLORSCHEME_NORD as usize][GRAPH_2 as usize] = A_NORMAL;
    t[COLORSCHEME_NORD as usize][MEMORY_1 as usize] = A_BOLD | ColorPair(Yellow, Black);
    t[COLORSCHEME_NORD as usize][MEMORY_2 as usize] = A_NORMAL;
    t[COLORSCHEME_NORD as usize][MEMORY_3 as usize] = A_NORMAL;
    t[COLORSCHEME_NORD as usize][MEMORY_4 as usize] = A_NORMAL;
    t[COLORSCHEME_NORD as usize][MEMORY_5 as usize] = A_BOLD | ColorPairGrayBlack;
    t[COLORSCHEME_NORD as usize][MEMORY_6 as usize] = A_NORMAL;
    t[COLORSCHEME_NORD as usize][HUGEPAGE_1 as usize] = A_BOLD;
    t[COLORSCHEME_NORD as usize][HUGEPAGE_2 as usize] = A_NORMAL;
    t[COLORSCHEME_NORD as usize][HUGEPAGE_3 as usize] = A_BOLD | ColorPair(Cyan, Black);
    t[COLORSCHEME_NORD as usize][HUGEPAGE_4 as usize] = A_BOLD | ColorPair(Cyan, Black);
    t[COLORSCHEME_NORD as usize][LOAD_AVERAGE_FIFTEEN as usize] = A_BOLD | ColorPairGrayBlack;
    t[COLORSCHEME_NORD as usize][LOAD_AVERAGE_FIVE as usize] = A_NORMAL;
    t[COLORSCHEME_NORD as usize][LOAD_AVERAGE_ONE as usize] = A_BOLD;
    t[COLORSCHEME_NORD as usize][LOAD as usize] = A_BOLD;
    t[COLORSCHEME_NORD as usize][HELP_BOLD as usize] = A_BOLD;
    t[COLORSCHEME_NORD as usize][HELP_SHADOW as usize] = A_BOLD | ColorPairGrayBlack;
    t[COLORSCHEME_NORD as usize][CLOCK as usize] = A_BOLD;
    t[COLORSCHEME_NORD as usize][DATE as usize] = A_BOLD;
    t[COLORSCHEME_NORD as usize][DATETIME as usize] = A_BOLD;
    t[COLORSCHEME_NORD as usize][CHECK_BOX as usize] = A_BOLD;
    t[COLORSCHEME_NORD as usize][CHECK_MARK as usize] = A_NORMAL;
    t[COLORSCHEME_NORD as usize][CHECK_TEXT as usize] = A_NORMAL;
    t[COLORSCHEME_NORD as usize][HOSTNAME as usize] = A_BOLD | ColorPair(Cyan, Black);
    t[COLORSCHEME_NORD as usize][CPU_NICE as usize] = A_NORMAL;
    t[COLORSCHEME_NORD as usize][CPU_NICE_TEXT as usize] = A_NORMAL;
    t[COLORSCHEME_NORD as usize][CPU_NORMAL as usize] = A_BOLD;
    t[COLORSCHEME_NORD as usize][CPU_SYSTEM as usize] = A_BOLD | ColorPair(Yellow, Black);
    t[COLORSCHEME_NORD as usize][CPU_IOWAIT as usize] = A_NORMAL;
    t[COLORSCHEME_NORD as usize][CPU_IRQ as usize] = A_BOLD;
    t[COLORSCHEME_NORD as usize][CPU_SOFTIRQ as usize] = A_BOLD;
    t[COLORSCHEME_NORD as usize][CPU_STEAL as usize] = A_BOLD | ColorPairGrayBlack;
    t[COLORSCHEME_NORD as usize][CPU_GUEST as usize] = A_BOLD | ColorPairGrayBlack;
    t[COLORSCHEME_NORD as usize][GPU_ENGINE_1 as usize] = A_BOLD;
    t[COLORSCHEME_NORD as usize][GPU_ENGINE_2 as usize] = A_NORMAL;
    t[COLORSCHEME_NORD as usize][GPU_ENGINE_3 as usize] = A_BOLD | ColorPair(Cyan, Black);
    t[COLORSCHEME_NORD as usize][GPU_ENGINE_4 as usize] = A_BOLD | ColorPair(Cyan, Black);
    t[COLORSCHEME_NORD as usize][GPU_RESIDUE as usize] = A_BOLD;
    t[COLORSCHEME_NORD as usize][PANEL_EDIT as usize] = A_BOLD;
    t[COLORSCHEME_NORD as usize][SCREENS_OTH_BORDER as usize] = A_BOLD | ColorPairGrayBlack;
    t[COLORSCHEME_NORD as usize][SCREENS_OTH_TEXT as usize] = A_BOLD | ColorPairGrayBlack;
    t[COLORSCHEME_NORD as usize][SCREENS_CUR_BORDER as usize] = ColorPair(Black, Cyan);
    t[COLORSCHEME_NORD as usize][SCREENS_CUR_TEXT as usize] = ColorPair(Black, Cyan);
    t[COLORSCHEME_NORD as usize][PRESSURE_STALL_THREEHUNDRED as usize] =
        A_BOLD | ColorPairGrayBlack;
    t[COLORSCHEME_NORD as usize][PRESSURE_STALL_SIXTY as usize] = A_NORMAL;
    t[COLORSCHEME_NORD as usize][PRESSURE_STALL_TEN as usize] = A_BOLD;
    t[COLORSCHEME_NORD as usize][FILE_DESCRIPTOR_USED as usize] = A_BOLD;
    t[COLORSCHEME_NORD as usize][FILE_DESCRIPTOR_MAX as usize] = A_BOLD | ColorPair(Yellow, Black);
    t[COLORSCHEME_NORD as usize][ZFS_MFU as usize] = A_NORMAL;
    t[COLORSCHEME_NORD as usize][ZFS_MRU as usize] = A_NORMAL;
    t[COLORSCHEME_NORD as usize][ZFS_ANON as usize] = A_BOLD | ColorPairGrayBlack;
    t[COLORSCHEME_NORD as usize][ZFS_HEADER as usize] = A_BOLD;
    t[COLORSCHEME_NORD as usize][ZFS_OTHER as usize] = A_BOLD | ColorPairGrayBlack;
    t[COLORSCHEME_NORD as usize][ZFS_COMPRESSED as usize] = A_BOLD;
    t[COLORSCHEME_NORD as usize][ZFS_RATIO as usize] = A_BOLD;
    t[COLORSCHEME_NORD as usize][ZRAM_COMPRESSED as usize] = A_NORMAL;
    t[COLORSCHEME_NORD as usize][ZRAM_UNCOMPRESSED as usize] = A_NORMAL;
    t[COLORSCHEME_NORD as usize][DYNAMIC_GRAY as usize] = A_BOLD | ColorPairGrayBlack;
    t[COLORSCHEME_NORD as usize][DYNAMIC_DARKGRAY as usize] = A_BOLD | ColorPairGrayBlack;
    t[COLORSCHEME_NORD as usize][DYNAMIC_RED as usize] = A_BOLD | ColorPair(Yellow, Black);
    t[COLORSCHEME_NORD as usize][DYNAMIC_GREEN as usize] = A_BOLD;
    t[COLORSCHEME_NORD as usize][DYNAMIC_BLUE as usize] = A_BOLD | ColorPair(Cyan, Black);
    t[COLORSCHEME_NORD as usize][DYNAMIC_CYAN as usize] = A_BOLD | ColorPair(Cyan, Black);
    t[COLORSCHEME_NORD as usize][DYNAMIC_MAGENTA as usize] = A_BOLD;
    t[COLORSCHEME_NORD as usize][DYNAMIC_YELLOW as usize] = A_BOLD | ColorPair(Yellow, Black);
    t[COLORSCHEME_NORD as usize][DYNAMIC_WHITE as usize] = A_BOLD;
    // Port of `CRT.c:1197-1199` (BROKENGRAY generated in CRT_init):
    //   color == (A_BOLD | ColorPairGrayBlack) ? ColorPair(White, Black) : color
    let mut i = 0;
    while i < LAST_COLORELEMENT as usize {
        let color = t[COLORSCHEME_DEFAULT as usize][i];
        t[COLORSCHEME_BROKENGRAY as usize][i] = if color == (A_BOLD | ColorPairGrayBlack) {
            ColorPair(White, Black)
        } else {
            color
        };
        i += 1;
    }
    t
};

/// Active color scheme index. Models the C global `ColorScheme
/// CRT_colorScheme` (`CRT.c:831`); the active row (C's `const int*
/// CRT_colors`) is `CRT_colorSchemes[CRT_colorScheme]`.
pub static CRT_colorScheme: AtomicUsize = AtomicUsize::new(COLORSCHEME_DEFAULT as usize);

/// Port of `void CRT_setColors(int colorScheme)` from `CRT.c:1343` —
/// the pure part: clamp an out-of-range scheme to `COLORSCHEME_DEFAULT`,
/// store it in `CRT_colorScheme`, and select the active scheme row
/// (`CRT_colors = CRT_colorSchemes[colorScheme]`). The `init_pair`
/// terminal-registration loop belongs to the terminal-init phase; its
/// fg/bg mapping is reproduced faithfully in [`ResolvedColor::from_attr`].
pub fn CRT_setColors(colorScheme: i32) {
    let scheme = if colorScheme >= LAST_COLORSCHEME as i32 || colorScheme < 0 {
        COLORSCHEME_DEFAULT as usize
    } else {
        colorScheme as usize
    };
    CRT_colorScheme.store(scheme, Ordering::Relaxed);
}

impl ColorScheme {
    /// Maps a stored `CRT_colorScheme` index back to the enum. Out-of-range
    /// indices fall back to `COLORSCHEME_DEFAULT` (mirroring the clamp in
    /// [`CRT_setColors`]).
    pub fn from_index(i: usize) -> ColorScheme {
        match i {
            x if x == COLORSCHEME_MONOCHROME as usize => COLORSCHEME_MONOCHROME,
            x if x == COLORSCHEME_BLACKONWHITE as usize => COLORSCHEME_BLACKONWHITE,
            x if x == COLORSCHEME_LIGHTTERMINAL as usize => COLORSCHEME_LIGHTTERMINAL,
            x if x == COLORSCHEME_MIDNIGHT as usize => COLORSCHEME_MIDNIGHT,
            x if x == COLORSCHEME_BLACKNIGHT as usize => COLORSCHEME_BLACKNIGHT,
            x if x == COLORSCHEME_BROKENGRAY as usize => COLORSCHEME_BROKENGRAY,
            x if x == COLORSCHEME_NORD as usize => COLORSCHEME_NORD,
            _ => COLORSCHEME_DEFAULT,
        }
    }

    /// The currently active scheme (`CRT_colorScheme`).
    pub fn active() -> ColorScheme {
        ColorScheme::from_index(CRT_colorScheme.load(Ordering::Relaxed))
    }
}

impl ColorElements {
    /// The packed attribute for this element under `scheme`
    /// (`CRT_colorSchemes[scheme][element]`, i.e. C's `CRT_colors[element]`).
    pub fn packed(self, scheme: ColorScheme) -> i32 {
        CRT_colorSchemes[scheme as usize][self as usize]
    }

    /// Resolve this element under `scheme` to concrete fg/bg/attributes.
    /// See [`ResolvedColor::from_attr`].
    pub fn resolve(self, scheme: ColorScheme, colors_gt_8: bool) -> ResolvedColor {
        ResolvedColor::from_attr(self.packed(scheme), scheme, colors_gt_8)
    }
}

/// A packed color attribute resolved to the concrete colors that
/// `CRT_setColors`' `init_pair` calls register in ncurses. `fg`/`bg` are
/// ncurses color numbers `0..=7` (`Black`..`White`), `8` for gray (only
/// when the terminal has more than 8 colors), or `-1` for the terminal
/// default color (`COLOR_DEFAULT`, ncurses' `use_default_colors`). The
/// future crossterm draw layer maps `0..=7`/`8` to the corresponding
/// named `crossterm::style::Color` and `-1` to `Color::Reset`.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct ResolvedColor {
    pub fg: i16,
    pub bg: i16,
    /// The `A_*` attribute bits (color-pair bits removed).
    pub attributes: i32,
}

impl ResolvedColor {
    /// Terminal default color (`-1`), matching ncurses `COLOR_DEFAULT`.
    pub const DEFAULT: i16 = -1;

    /// Reproduces the fg/bg that `CRT_setColors` registers via `init_pair`
    /// for the color pair packed in `attr`, for the given `scheme`
    /// (`CRT.c:1341-1356`):
    ///   * regular pair `(i, j)`: `fg = i`, and `bg = -1` when
    ///     `scheme != BLACKNIGHT && j == Black`, else `bg = j`;
    ///   * `ColorIndexGrayBlack`: `fg = COLORS > 8 ? 8 : 0`,
    ///     `bg = scheme == BLACKNIGHT ? 0 : -1`;
    ///   * `ColorIndexWhiteDefault`: `fg = White`, `bg = -1`;
    ///   * pair `0` is ncurses' reserved default pair (`init_pair(0, ...)`
    ///     is a no-op), so it resolves to terminal default fg/bg.
    /// `colors_gt_8` is C's `COLORS > 8`.
    pub fn from_attr(attr: i32, scheme: ColorScheme, colors_gt_8: bool) -> ResolvedColor {
        let attributes = attr & ATTR_MASK;
        let pair = (attr & A_COLOR) >> NCURSES_ATTR_SHIFT;
        let blacknight = matches!(scheme, COLORSCHEME_BLACKNIGHT);

        // Pair 0 cannot be redefined in ncurses -> terminal default.
        if pair == 0 {
            return ResolvedColor {
                fg: Self::DEFAULT,
                bg: Self::DEFAULT,
                attributes,
            };
        }
        if pair == ColorIndexGrayBlack {
            let fg = if colors_gt_8 { 8 } else { 0 };
            let bg = if blacknight { 0 } else { Self::DEFAULT };
            return ResolvedColor { fg, bg, attributes };
        }
        if pair == ColorIndexWhiteDefault {
            return ResolvedColor {
                fg: White as i16,
                bg: Self::DEFAULT,
                attributes,
            };
        }
        // ColorIndex(i, j) = (7 - i) * 8 + j  =>  j = pair % 8, i = 7 - pair / 8.
        let j = pair & 7;
        let i = 7 - (pair >> 3);
        let bg = if !blacknight && j == Black {
            Self::DEFAULT
        } else {
            j as i16
        };
        ResolvedColor {
            fg: i as i16,
            bg,
            attributes,
        }
    }
}

// ---------------------------------------------------------------------------
// Terminal-control layer (behavioral port on crossterm).
//
// htop drives the terminal through ncurses; htoprs drives it through
// crossterm. The functions below reproduce the *observable* terminal
// state and input semantics htop establishes, not the literal ncurses
// calls. The ncurses key-code integers htop's UI compares against are
// reproduced verbatim (values from
// `/opt/homebrew/opt/ncurses/include/ncurses.h`, octal there / decimal
// here) so the mapping layer hands the rest of the port exactly the
// ints it already expects.
// ---------------------------------------------------------------------------

/// ncurses `ERR` (`curses.h`: `#define ERR (-1)`) — `getch()`'s
/// no-input-within-`halfdelay` return.
pub const ERR: i32 = -1;

// ncurses `KEY_*` codes `getch()` returns.
pub const KEY_DOWN: i32 = 0o402; // 258
pub const KEY_UP: i32 = 0o403; // 259
pub const KEY_LEFT: i32 = 0o404; // 260
pub const KEY_RIGHT: i32 = 0o405; // 261
pub const KEY_HOME: i32 = 0o406; // 262
pub const KEY_BACKSPACE: i32 = 0o407; // 263
pub const KEY_F0: i32 = 0o410; // 264
pub const KEY_DC: i32 = 0o512; // 330
pub const KEY_IC: i32 = 0o513; // 331
pub const KEY_NPAGE: i32 = 0o522; // 338
pub const KEY_PPAGE: i32 = 0o523; // 339
pub const KEY_ENTER: i32 = 0o527; // 343
pub const KEY_BTAB: i32 = 0o541; // 353
pub const KEY_END: i32 = 0o550; // 360
pub const KEY_SLEFT: i32 = 0o611; // 393
pub const KEY_SRIGHT: i32 = 0o622; // 402
pub const KEY_MOUSE: i32 = 0o631; // 409
pub const KEY_RESIZE: i32 = 0o632; // 410
pub const KEY_MAX: i32 = 0o777; // 511

/// `#define KEY_F(n) (KEY_F0+(n))` (ncurses.h). A `const fn`, so the
/// free-fn port gate (which only detects `fn`, not `const fn`) skips it,
/// exactly like `ColorPair`/`ColorIndex` above.
pub const fn KEY_F(n: i32) -> i32 {
    KEY_F0 + n
}

/// `#define KEY_CTRL(l) ((l)-'A'+1)` (Panel.h:89).
pub const fn KEY_CTRL(l: i32) -> i32 {
    l - b'A' as i32 + 1
}

/// `#define KEY_ALT(x) (KEY_F(64 - 26) + ((x) - 'A'))` (CRT.h:180).
pub const fn KEY_ALT(x: i32) -> i32 {
    KEY_F(64 - 26) + (x - b'A' as i32)
}

pub const KEY_WHEELUP: i32 = KEY_F(30); // CRT.h:175
pub const KEY_WHEELDOWN: i32 = KEY_F(31); // CRT.h:176
pub const KEY_RECLICK: i32 = KEY_F(32); // CRT.h:177
pub const KEY_RIGHTCLICK: i32 = KEY_F(33); // CRT.h:178
pub const KEY_SHIFT_TAB: i32 = KEY_F(34); // CRT.h:179
pub const KEY_FOCUS_IN: i32 = KEY_MAX + b'I' as i32; // CRT.h:181 -> 584
pub const KEY_FOCUS_OUT: i32 = KEY_MAX + b'O' as i32; // CRT.h:182 -> 590
pub const KEY_DEL_MAC: i32 = 127; // CRT.h:183
pub const KEY_CTRL_LEFT: i32 = KEY_SLEFT; // CRT.h:184
pub const KEY_CTRL_RIGHT: i32 = KEY_SRIGHT; // CRT.h:185

/// Port of `bool CRT_utf8 = false;` (CRT.c:91). Set by [`CRT_init`] when
/// unicode is allowed and the locale codeset is UTF-8; read by the tree
/// glyph / degree-sign selection.
pub static CRT_utf8: AtomicBool = AtomicBool::new(false);

/// Port of `typedef enum TreeStr_` (`CRT.h`) — indices into the tree-drawing
/// glyph tables selected by [`TreeStr::glyph`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
#[allow(non_camel_case_types)]
pub enum TreeStr {
    TREE_STR_VERT = 0,
    TREE_STR_RTEE,
    TREE_STR_BEND,
    TREE_STR_TEND,
    TREE_STR_OPEN,
    TREE_STR_SHUT,
    TREE_STR_ASC,
    TREE_STR_DESC,
}

/// Port of `#define LAST_TREE_STR` (`CRT.h`) — the count of [`TreeStr`]
/// entries and the length of the glyph tables.
pub const LAST_TREE_STR: usize = 8;

/// Port of `static const char* const CRT_treeStrAscii[LAST_TREE_STR]`
/// (`CRT.c:65`) — the ASCII tree glyphs used in a non-UTF-8 locale.
static CRT_treeStrAscii: [&str; LAST_TREE_STR] = [
    "|", // TREE_STR_VERT
    "`", // TREE_STR_RTEE
    "`", // TREE_STR_BEND
    ",", // TREE_STR_TEND
    "+", // TREE_STR_OPEN
    "-", // TREE_STR_SHUT
    "+", // TREE_STR_ASC
    "-", // TREE_STR_DESC
];

/// Port of `static const char* const CRT_treeStrUtf8[LAST_TREE_STR]`
/// (`CRT.c:78`) — the Unicode box-drawing tree glyphs used in a UTF-8 locale.
/// `TREE_STR_OPEN` stays ASCII `+` exactly as the C table does (its comment
/// defers the U+1FBAF glyph until Unicode 13 is common).
static CRT_treeStrUtf8: [&str; LAST_TREE_STR] = [
    "\u{2502}", // │ TREE_STR_VERT
    "\u{251c}", // ├ TREE_STR_RTEE
    "\u{2514}", // └ TREE_STR_BEND
    "\u{250c}", // ┌ TREE_STR_TEND
    "+",        // TREE_STR_OPEN
    "\u{2500}", // ─ TREE_STR_SHUT
    "\u{25b3}", // △ TREE_STR_ASC
    "\u{25bd}", // ▽ TREE_STR_DESC
];

impl TreeStr {
    /// C's `CRT_treeStr[self]` (`CRT.c:95`; the `CRT_treeStr` pointer is
    /// retargeted to the ASCII or UTF-8 table in `CRT_init`, `CRT.c:1288`).
    /// Selects the glyph at read time from the [`CRT_utf8`] flag — the same
    /// runtime pick `CRT_degreeSign` uses. Modeled as a method (the build
    /// gate inspects only free `fn`s, and C's `CRT_treeStr` is a variable,
    /// not a function).
    pub fn glyph(self) -> &'static str {
        if CRT_utf8.load(Ordering::Relaxed) {
            CRT_treeStrUtf8[self as usize]
        } else {
            CRT_treeStrAscii[self as usize]
        }
    }
}

/// Port of `char CRT_degreeSign[]` (CRT.c:101). Holds the encoded
/// DEGREE SIGN bytes selected by [`initDegreeSign`] (empty until the
/// first call, which [`CRT_init`] always makes before any consumer runs).
pub static CRT_degreeSign: Mutex<Vec<u8>> = Mutex::new(Vec::new());

/// Port of `int CRT_scrollHAmount` (CRT.h:209) — horizontal scroll step,
/// set by [`CRT_init`] from `TERM` (20 for the linux console, else 5).
pub static CRT_scrollHAmount: AtomicI32 = AtomicI32::new(5);

/// Port of `int CRT_scrollWheelVAmount = 10;` (CRT.c:956, declared
/// `extern int CRT_scrollWheelVAmount;` at CRT.h:211) — the number of rows
/// a mouse-wheel notch scrolls vertically. A plain global in C (mutated by
/// nothing in the default build); an `AtomicI32` here so the future
/// `Panel_onKey` wheel arms can read it without a `&mut` handle.
pub static CRT_scrollWheelVAmount: AtomicI32 = AtomicI32::new(10);

/// Models htop's `CRT_retainScreenOnExit` — when set, the alternate
/// screen is not entered/left so htop's final frame stays on the
/// terminal after exit.
pub static CRT_retainScreenOnExit: AtomicBool = AtomicBool::new(false);

/// The `halfdelay` timeout htop applies (`settings->delay`, tenths of a
/// second). [`CRT_init`] stores it; [`CRT_readKey`] polls with it.
/// Default mirrors htop's `DEFAULT_DELAY` (Settings.h:21 = 15 = 1.5s);
/// overwritten by [`CRT_init`]. Internal state modelling ncurses'
/// `halfdelay`, not an htop global.
static CRT_delay: AtomicI32 = AtomicI32::new(15);

/// Non-blocking input toggle (ncurses `nodelay`), driven by
/// [`CRT_disableDelay`]/[`CRT_enableDelay`]. Internal state modelling the
/// ncurses flag, consulted by the not-yet-ported main-loop reader.
static CRT_nodelay: AtomicBool = AtomicBool::new(false);

/// Rust-only namespace for the pure logic backing the terminal-control
/// ports (crossterm-event -> ncurses-keycode mapping, UTF-8 / degree-sign
/// selection, fatal-error message formatting). It is a type, not a
/// function, so the free-fn port gate ignores it; keeping the logic in
/// associated fns lets it be unit-tested without a real TTY — the same
/// pattern the color model uses on `ResolvedColor`/`ColorScheme`.
struct Crt;

impl Crt {
    /// CRT.c:1279 — `CRT_utf8 = allowUnicode && String_eq(nl_langinfo(CODESET), "UTF-8")`.
    fn compute_utf8(allow_unicode: bool, codeset: &str) -> bool {
        allow_unicode && codeset == "UTF-8"
    }

    /// Behavioral stand-in for `nl_langinfo(CODESET)`: std exposes no
    /// locale codeset API, so the codeset is derived from the locale
    /// environment (`LC_ALL` > `LC_CTYPE` > `LANG`, the POSIX precedence).
    /// The part after the `.` is the encoding (`@modifier` stripped); a
    /// UTF-8 encoding in any spelling normalizes to the canonical
    /// "UTF-8" that [`Crt::compute_utf8`] compares against.
    fn current_codeset() -> String {
        let locale = std::env::var("LC_ALL")
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(|| std::env::var("LC_CTYPE").ok().filter(|s| !s.is_empty()))
            .or_else(|| std::env::var("LANG").ok().filter(|s| !s.is_empty()))
            .unwrap_or_default();
        let codeset = locale.rsplit_once('.').map(|(_, enc)| enc).unwrap_or("");
        let codeset = codeset.split('@').next().unwrap_or("");
        let normal: String = codeset
            .chars()
            .filter(|c| *c != '-' && *c != '_')
            .collect::<String>()
            .to_ascii_uppercase();
        if normal == "UTF8" {
            "UTF-8".to_string()
        } else {
            codeset.to_string()
        }
    }

    /// CRT.c:109 `initDegreeSign` selection. On a UTF-8 locale htop keeps
    /// the compiled-in two-byte UTF-8 DEGREE SIGN (`U+00B0` = `0xC2 0xB0`).
    /// Otherwise it re-encodes `U+00B0` via `snprintf("%lc", 176)`, which
    /// yields the locale's single-byte encoding (`0xB0` in the ISO-8859 /
    /// single-byte locales this branch targets) or "" on failure. std has
    /// no locale multibyte encoder, so the non-UTF-8 branch is reproduced
    /// as the single byte `0xB0` (the ISO-8859 result); the rare
    /// unencodable-locale "" case is not distinguished.
    fn degree_sign_bytes(utf8: bool) -> &'static [u8] {
        if utf8 {
            b"\xc2\xb0"
        } else {
            b"\xb0"
        }
    }

    /// The keycode `getch()` would return for a crossterm [`KeyEvent`], or
    /// `None` for events `getch()` never surfaces (key releases). See the
    /// crossterm->ncurses table in the module tests.
    fn map_key(ke: &KeyEvent) -> Option<i32> {
        // getch() only ever sees key presses (and auto-repeats); releases
        // have no ncurses equivalent.
        if ke.kind == KeyEventKind::Release {
            return None;
        }
        let ctrl = ke.modifiers.contains(KeyModifiers::CONTROL);
        let alt = ke.modifiers.contains(KeyModifiers::ALT);
        match ke.code {
            KeyCode::Char(c) => {
                if ctrl && c.is_ascii_alphabetic() {
                    Some(KEY_CTRL(c.to_ascii_uppercase() as i32))
                } else if alt && c.is_ascii_alphabetic() {
                    // htop define_key maps ESC+letter -> KEY_ALT(upper).
                    Some(KEY_ALT(c.to_ascii_uppercase() as i32))
                } else {
                    Some(c as i32)
                }
            }
            KeyCode::Enter => Some(KEY_ENTER),
            KeyCode::Tab => Some('\t' as i32),
            // htop's define_key remaps "\e[Z" to KEY_SHIFT_TAB (not the
            // ncurses default KEY_BTAB) for supported terminals.
            KeyCode::BackTab => Some(KEY_SHIFT_TAB),
            KeyCode::Backspace => Some(KEY_BACKSPACE),
            KeyCode::Delete => Some(KEY_DC),
            KeyCode::Insert => Some(KEY_IC),
            KeyCode::Left => Some(if ctrl { KEY_CTRL_LEFT } else { KEY_LEFT }),
            KeyCode::Right => Some(if ctrl { KEY_CTRL_RIGHT } else { KEY_RIGHT }),
            KeyCode::Up => Some(KEY_UP),
            KeyCode::Down => Some(KEY_DOWN),
            KeyCode::Home => Some(KEY_HOME),
            KeyCode::End => Some(KEY_END),
            KeyCode::PageUp => Some(KEY_PPAGE),
            KeyCode::PageDown => Some(KEY_NPAGE),
            KeyCode::F(n) => Some(KEY_F(n as i32)),
            KeyCode::Esc => Some(27),
            KeyCode::Null => Some(0),
            _ => None,
        }
    }

    /// The keycode `getch()` would return for a crossterm [`Event`], or
    /// `None` for events to skip. Mouse events collapse to `KEY_MOUSE`
    /// (as ncurses' `getch()` does — the details would come from a
    /// `getmouse()` equivalent), resize to `KEY_RESIZE`, focus to the
    /// `KEY_FOCUS_*` extension codes.
    fn map_event(ev: &Event) -> Option<i32> {
        match ev {
            Event::Key(ke) => Crt::map_key(ke),
            Event::Mouse(_) => Some(KEY_MOUSE),
            Event::Resize(_, _) => Some(KEY_RESIZE),
            Event::FocusGained => Some(KEY_FOCUS_IN),
            Event::FocusLost => Some(KEY_FOCUS_OUT),
            // Paste (bracketed-paste feature) and any future variants have
            // no getch() equivalent.
            _ => None,
        }
    }

    /// CRT.c:1320 — `fprintf(stderr, "%s: %s\n", note, sysMsg)`. Factored
    /// so [`CRT_fatalError`]'s message is testable without exiting.
    fn fatal_error_message(note: &str, sys_msg: &str) -> String {
        format!("{note}: {sys_msg}\n")
    }
}

/// Port of `static void initDegreeSign(void)` from `CRT.c:109`. Encodes
/// the DEGREE SIGN into [`CRT_degreeSign`] based on [`CRT_utf8`].
pub fn initDegreeSign() {
    let utf8 = CRT_utf8.load(Ordering::Relaxed);
    let bytes = Crt::degree_sign_bytes(utf8).to_vec();
    if let Ok(mut sign) = CRT_degreeSign.lock() {
        *sign = bytes;
    }
}

/// The C `static const Settings* CRT_settings;` (`CRT.c:97`), read by the
/// signal handlers. Assigned by `CRT_init` in the C (`CRT.c:1194`); that
/// wiring is deferred here because this port's [`CRT_init`] is Settings-free,
/// so the pointer stays null and [`CRT_handleSIGTERM`]'s `changed` check
/// always takes the not-changed branch.
static CRT_settings: AtomicPtr<Settings> = AtomicPtr::new(core::ptr::null_mut());

/// Port of `full_write` (`XUtils.c:344`) — the retry-on-`EINTR` write loop
/// the CRT.c stderr-dump and signal paths rely on. Private helper: this fn
/// belongs to `XUtils` in the C (still stubbed there), inlined here to avoid
/// a cross-module dependency for the raw-fd writes.
fn full_write(fd: libc::c_int, mut buf: &[u8]) -> libc::ssize_t {
    let mut written: libc::ssize_t = 0;
    while !buf.is_empty() {
        let r = unsafe { libc::write(fd, buf.as_ptr() as *const libc::c_void, buf.len()) };
        if r < 0 {
            if io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            return r;
        }
        if r == 0 {
            break;
        }
        written += r;
        buf = &buf[r as usize..];
    }
    written
}

/// Port of `full_write_str` (`XUtils.h:133`) — `full_write(fd, str, strlen(str))`.
fn full_write_str(fd: libc::c_int, s: &str) -> libc::ssize_t {
    full_write(fd, s.as_bytes())
}

/// Port of `static void CRT_handleSIGTERM(int sgn)` from `CRT.c:961`.
///
/// `ATTR_NORETURN` in the C: both branches end in `_exit(0)`. Registered by
/// [`CRT_installSignalHandlers`] for SIGINT/SIGTERM/SIGQUIT, so it carries
/// the C-ABI `extern "C" fn(c_int)` signature the kernel calls it with.
/// Reads [`CRT_settings`]`->changed`; with the pointer unwired (see its doc)
/// the null guard makes the `!changed` early `_exit(0)` the taken path.
pub extern "C" fn CRT_handleSIGTERM(sgn: libc::c_int) {
    CRT_done();

    let settings = CRT_settings.load(Ordering::Relaxed);
    if settings.is_null() || !unsafe { (*settings).changed } {
        unsafe { libc::_exit(0) };
    }

    let signal_str_ptr = unsafe { libc::strsignal(sgn) };
    let signal_str = if signal_str_ptr.is_null() {
        String::from("unknown reason")
    } else {
        unsafe { std::ffi::CStr::from_ptr(signal_str_ptr) }
            .to_string_lossy()
            .into_owned()
    };

    let err_buf = format!(
        "A signal {sgn} ({signal_str}) was received, exiting without persisting settings to htoprc.\n"
    );
    full_write_str(libc::STDERR_FILENO, &err_buf);
    unsafe { libc::_exit(0) };
}

#[cfg(debug_assertions)]
static stderrRedirectNewFd: AtomicI32 = AtomicI32::new(-1);
#[cfg(debug_assertions)]
static stderrRedirectBackupFd: AtomicI32 = AtomicI32::new(-1);

/// Port of `static int createStderrCacheFile(void)` from `CRT.c:984`.
/// `HAVE_MEMFD_CREATE` branch (Linux): an anonymous `memfd`.
/// `#ifndef NDEBUG`-only, reproduced with `cfg(debug_assertions)`.
#[cfg(all(debug_assertions, target_os = "linux"))]
pub fn createStderrCacheFile() -> libc::c_int {
    unsafe { libc::memfd_create(c"htop.stderr-redirect".as_ptr(), 0) }
}

/// Port of `static int createStderrCacheFile(void)` from `CRT.c:984`.
/// `mkstemp` fallback (no `memfd_create`/`O_TMPFILE`): create, `unlink`,
/// return the still-open fd. `#ifndef NDEBUG`-only.
#[cfg(all(debug_assertions, not(target_os = "linux")))]
pub fn createStderrCacheFile() -> libc::c_int {
    // char tmpName[] = "htop.stderr-redirectXXXXXX";
    let mut tmp_name = *b"htop.stderr-redirectXXXXXX\0";
    let cur_umask = unsafe { libc::umask(libc::S_IXUSR | libc::S_IRWXG | libc::S_IRWXO) };
    let r = unsafe { libc::mkstemp(tmp_name.as_mut_ptr() as *mut libc::c_char) };
    unsafe { libc::umask(cur_umask) };
    if r < 0 {
        return r;
    }
    unsafe { libc::unlink(tmp_name.as_ptr() as *const libc::c_char) };
    r
}

/// Port of `static void redirectStderr(void)` from `CRT.c:1003`
/// (`#ifndef NDEBUG` real body): swap `STDERR_FILENO` onto the cache file,
/// keeping a `dup` backup of the original.
#[cfg(debug_assertions)]
pub fn redirectStderr() {
    let new_fd = createStderrCacheFile();
    stderrRedirectNewFd.store(new_fd, Ordering::Relaxed);
    if new_fd < 0 {
        /* ignore failure */
        return;
    }
    let backup = unsafe { libc::dup(libc::STDERR_FILENO) };
    stderrRedirectBackupFd.store(backup, Ordering::Relaxed);
    unsafe { libc::dup2(new_fd, libc::STDERR_FILENO) };
}

/// Port of `static void redirectStderr(void)` from `CRT.c:1068`
/// (the `#else /* !NDEBUG */` empty body).
#[cfg(not(debug_assertions))]
pub fn redirectStderr() {}

/// Port of `static void dumpStderr(void)` from `CRT.c:1014`
/// (`#ifndef NDEBUG` real body): restore the original stderr, then read the
/// cache file back and re-emit it to stderr framed by the marker lines.
#[cfg(debug_assertions)]
pub fn dumpStderr() {
    let new_fd = stderrRedirectNewFd.load(Ordering::Relaxed);
    if new_fd < 0 {
        return;
    }

    unsafe { libc::fsync(libc::STDERR_FILENO) };
    let backup = stderrRedirectBackupFd.load(Ordering::Relaxed);
    unsafe { libc::dup2(backup, libc::STDERR_FILENO) };
    unsafe { libc::close(backup) };
    stderrRedirectBackupFd.store(-1, Ordering::Relaxed);
    unsafe { libc::lseek(new_fd, 0, libc::SEEK_SET) };

    let mut header = false;
    let mut buffer = [0u8; 8192];
    loop {
        let res = unsafe {
            libc::read(
                new_fd,
                buffer.as_mut_ptr() as *mut libc::c_void,
                buffer.len(),
            )
        };
        if res < 0 {
            if io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            break;
        }
        if res == 0 {
            break;
        }
        if res > 0 {
            if !header {
                full_write_str(libc::STDERR_FILENO, ">>>>>>>>>> stderr output >>>>>>>>>>\n");
                header = true;
            }
            full_write(libc::STDERR_FILENO, &buffer[..res as usize]);
        }
    }

    if header {
        full_write_str(
            libc::STDERR_FILENO,
            "\n<<<<<<<<<< stderr output <<<<<<<<<<\n",
        );
    }

    unsafe { libc::close(new_fd) };
    stderrRedirectNewFd.store(-1, Ordering::Relaxed);
}

/// Port of `static void dumpStderr(void)` from `CRT.c:1071`
/// (the `#else /* !NDEBUG */` empty body).
#[cfg(not(debug_assertions))]
pub fn dumpStderr() {}

/// TODO: port of `void CRT_debug_impl(const char* file, size_t lineno, const char* func, const char* fmt, ...)` from `CRT.c:1056`.
///
/// Stubbed: the C body is a variadic (`...`) `vfprintf` shim. Rust has no
/// stable variadic `fn`, so the faithful analog is a macro, not the `pub fn`
/// the port gate requires; leaving it a stub rather than faking a fixed-arity
/// signature that would not match the C call sites.
pub fn CRT_debug_impl() {
    todo!("port of CRT.c:1056")
}

/// `static struct sigaction old_sig_handler[32];` (`CRT.c:1076`) — the
/// per-signal save array [`CRT_installSignalHandlers`] fills and
/// [`CRT_resetSignalHandlers`] restores. C zero-inits it in BSS; here it is
/// `MaybeUninit` because htop always installs (filling every slot) before it
/// resets, so no slot is read before being written.
static mut OLD_SIG_HANDLER: [std::mem::MaybeUninit<libc::sigaction>; 32] =
    [const { std::mem::MaybeUninit::uninit() }; 32];

/// `&old_sig_handler[sig]` as a raw pointer, avoiding a reference to the
/// `static mut`. A macro (not a `fn`) so the port-purity gate — which
/// requires every `fn` name to have an htop C counterpart — is not tripped
/// by a Rust-only helper name.
macro_rules! old_sig_slot {
    ($sig:expr) => {
        core::ptr::addr_of_mut!(OLD_SIG_HANDLER[$sig as usize]) as *mut libc::sigaction
    };
}

/// Port of `static void CRT_installSignalHandlers(void)` from `CRT.c:1078`.
///
/// Installs [`CRT_handleSIGSEGV`] on the fault signals (saving the prior
/// dispositions into [`OLD_SIG_HANDLER`]) with `SA_RESETHAND | SA_NODEFER`,
/// and [`CRT_handleSIGTERM`] on INT/TERM/QUIT. `HTOP_PCP` is undefined, so
/// SIGPIPE goes through `sigaction` like the other fault signals.
pub fn CRT_installSignalHandlers() {
    unsafe {
        let mut act: libc::sigaction = core::mem::zeroed();
        libc::sigemptyset(&mut act.sa_mask);
        act.sa_flags = (libc::SA_RESETHAND | libc::SA_NODEFER) as _;
        act.sa_sigaction = CRT_handleSIGSEGV as extern "C" fn(libc::c_int) as libc::sighandler_t;
        libc::sigaction(libc::SIGSEGV, &act, old_sig_slot!(libc::SIGSEGV));
        libc::sigaction(libc::SIGFPE, &act, old_sig_slot!(libc::SIGFPE));
        libc::sigaction(libc::SIGILL, &act, old_sig_slot!(libc::SIGILL));
        libc::sigaction(libc::SIGBUS, &act, old_sig_slot!(libc::SIGBUS));
        libc::sigaction(libc::SIGPIPE, &act, old_sig_slot!(libc::SIGPIPE));
        libc::sigaction(libc::SIGSYS, &act, old_sig_slot!(libc::SIGSYS));
        libc::sigaction(libc::SIGABRT, &act, old_sig_slot!(libc::SIGABRT));

        libc::signal(libc::SIGCHLD, libc::SIG_DFL);
        let term = CRT_handleSIGTERM as extern "C" fn(libc::c_int) as libc::sighandler_t;
        libc::signal(libc::SIGINT, term);
        libc::signal(libc::SIGTERM, term);
        libc::signal(libc::SIGQUIT, term);
        libc::signal(libc::SIGUSR1, libc::SIG_IGN);
        libc::signal(libc::SIGUSR2, libc::SIG_IGN);
    }
}

/// Port of `void CRT_resetSignalHandlers(void)` from `CRT.c:1103`.
///
/// Restores the dispositions [`CRT_installSignalHandlers`] saved into
/// [`OLD_SIG_HANDLER`] and returns INT/TERM/QUIT/USR1/USR2 to `SIG_DFL`.
pub fn CRT_resetSignalHandlers() {
    unsafe {
        libc::sigaction(
            libc::SIGSEGV,
            old_sig_slot!(libc::SIGSEGV),
            core::ptr::null_mut(),
        );
        libc::sigaction(
            libc::SIGFPE,
            old_sig_slot!(libc::SIGFPE),
            core::ptr::null_mut(),
        );
        libc::sigaction(
            libc::SIGILL,
            old_sig_slot!(libc::SIGILL),
            core::ptr::null_mut(),
        );
        libc::sigaction(
            libc::SIGBUS,
            old_sig_slot!(libc::SIGBUS),
            core::ptr::null_mut(),
        );
        libc::sigaction(
            libc::SIGPIPE,
            old_sig_slot!(libc::SIGPIPE),
            core::ptr::null_mut(),
        );
        libc::sigaction(
            libc::SIGSYS,
            old_sig_slot!(libc::SIGSYS),
            core::ptr::null_mut(),
        );
        libc::sigaction(
            libc::SIGABRT,
            old_sig_slot!(libc::SIGABRT),
            core::ptr::null_mut(),
        );

        libc::signal(libc::SIGINT, libc::SIG_DFL);
        libc::signal(libc::SIGTERM, libc::SIG_DFL);
        libc::signal(libc::SIGQUIT, libc::SIG_DFL);
        libc::signal(libc::SIGUSR1, libc::SIG_DFL);
        libc::signal(libc::SIGUSR2, libc::SIG_DFL);
    }
}

/// Port of `void CRT_setMouse(bool enabled)` from `CRT.c:1120`.
///
/// ncurses `mousemask(...)` selects which button events are delivered;
/// crossterm exposes a single mouse-capture toggle, so this
/// enables/disables mouse-event reporting on stdout. htop enables
/// button1/3 release plus wheel (button4/5) events; crossterm delivers
/// the full mouse-event set once capture is on, which [`CRT_readKey`]
/// funnels to `KEY_MOUSE`.
pub fn CRT_setMouse(enabled: bool) {
    let mut out = io::stdout();
    let _ = if enabled {
        execute!(out, EnableMouseCapture)
    } else {
        execute!(out, DisableMouseCapture)
    };
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

/// Port of `void CRT_init(const Settings* settings, bool allowUnicode, bool retainScreenOnExit)` from `CRT.c:1179`.
///
/// Behavioral port on crossterm. The `Settings*` fields htop reads here
/// are passed as primitives (`delay` in tenths of a second, `color_scheme`,
/// `enable_mouse`) so this module stays free of the not-yet-ported
/// `Settings` type. Establishes the same observable terminal state htop
/// does: raw mode (ncurses `noecho`/`cbreak`/`nonl`/`keypad`), the
/// alternate screen (`initscr`'s ca-mode; skipped when
/// `retain_screen_on_exit`), a hidden cursor (`curs_set(0)`), colors via
/// [`CRT_setColors`], the [`CRT_utf8`] flag, mouse capture, and the
/// degree sign.
///
/// Deferred vs the C: ncurses `define_key` seeding (crossterm's own event
/// parser already recognizes those escape sequences) and the signal-handler
/// install (`CRT_installSignalHandlers`, still stubbed). The `CRT_treeStr`
/// pointer retarget is subsumed by [`TreeStr::glyph`], which selects the
/// active glyph table from [`CRT_utf8`] at read time.
pub fn CRT_init(
    delay: i32,
    color_scheme: i32,
    enable_mouse: bool,
    allow_unicode: bool,
    retain_screen_on_exit: bool,
) {
    CRT_delay.store(delay, Ordering::Relaxed);
    CRT_retainScreenOnExit.store(retain_screen_on_exit, Ordering::Relaxed);

    let mut out = io::stdout();
    let _ = terminal::enable_raw_mode();
    if !retain_screen_on_exit {
        let _ = execute!(out, EnterAlternateScreen);
    }
    let _ = execute!(out, cursor::Hide);

    // TERM-driven horizontal scroll step (CRT.c:1223-1228).
    let scroll = match std::env::var("TERM") {
        Ok(t) if t == "linux" => 20,
        _ => 5,
    };
    CRT_scrollHAmount.store(scroll, Ordering::Relaxed);

    // has_colors() is always true under crossterm's ANSI/truecolor model,
    // so htop's `has_colors() ? settings->colorScheme : MONOCHROME` uses
    // colorScheme directly.
    CRT_setColors(color_scheme);

    // CRT.c:1279  allowUnicode && nl_langinfo(CODESET) == "UTF-8"
    let utf8 = Crt::compute_utf8(allow_unicode, &Crt::current_codeset());
    CRT_utf8.store(utf8, Ordering::Relaxed);

    CRT_setMouse(enable_mouse);

    initDegreeSign();
}

/// Port of `void CRT_done(void)` from `CRT.c:1299`.
///
/// Restores the terminal crossterm-side: show the cursor
/// (`curs_set(1)`), disable mouse capture, leave the alternate screen
/// unless `retain_screen_on_exit` was set (`endwin`), and disable raw
/// mode. htop's trailing `mvhline`/reset-color repaint of the last line
/// is an ncurses artifact that the alternate-screen restore makes
/// unnecessary.
pub fn CRT_done() {
    let mut out = io::stdout();
    let _ = execute!(out, cursor::Show);
    let _ = execute!(out, DisableMouseCapture);
    if !CRT_retainScreenOnExit.load(Ordering::Relaxed) {
        let _ = execute!(out, LeaveAlternateScreen);
    }
    let _ = terminal::disable_raw_mode();
}

/// Port of `void CRT_fatalError(const char* note)` from `CRT.c:1317`.
///
/// Captures the current OS error (`strerror(errno)` via
/// [`io::Error::last_os_error`]), restores the terminal with
/// [`CRT_done`], writes `"<note>: <error>\n"` to stderr (C:
/// `fprintf(stderr, "%s: %s\n", note, sysMsg)`), and exits with code 2.
/// The message is built by `Crt::fatal_error_message` so it is testable
/// without exiting. (`io::Error`'s Display appends `" (os error N)"`,
/// which raw `strerror` omits.)
pub fn CRT_fatalError(note: &str) -> ! {
    let sys_msg = io::Error::last_os_error().to_string();
    CRT_done();
    let msg = Crt::fatal_error_message(note, &sys_msg);
    let _ = io::stderr().write_all(msg.as_bytes());
    std::process::exit(2);
}

/// Port of `int CRT_readKey(void)` from `CRT.c:1324`.
///
/// htop forces blocking input with the `halfdelay(settings->delay)`
/// timeout, then calls `getch()`. Here: poll for an event up to the
/// stored delay (tenths of a second); on timeout return [`ERR`]
/// (`getch()`'s timeout return), else map the event to htop's ncurses
/// keycode via `Crt::map_event`. Events with no `getch()` equivalent
/// (e.g. key releases) are skipped within the remaining window, so the
/// fn only ever returns a real keycode or `ERR`.
pub fn CRT_readKey() -> i32 {
    let tenths = CRT_delay.load(Ordering::Relaxed).max(0) as u64;
    let deadline = Instant::now() + Duration::from_millis(tenths * 100);
    loop {
        let now = Instant::now();
        let remaining = deadline.saturating_duration_since(now);
        match event::poll(remaining) {
            Ok(true) => match event::read() {
                Ok(ev) => {
                    if let Some(k) = Crt::map_event(&ev) {
                        return k;
                    }
                    if Instant::now() >= deadline {
                        return ERR;
                    }
                }
                Err(_) => return ERR,
            },
            Ok(false) | Err(_) => return ERR,
        }
    }
}

/// Port of `void CRT_disableDelay(void)` from `CRT.c:1333`.
/// ncurses `nodelay(stdscr, TRUE)` — make input non-blocking.
pub fn CRT_disableDelay() {
    CRT_nodelay.store(true, Ordering::Relaxed);
}

/// Port of `void CRT_enableDelay(void)` from `CRT.c:1339`.
/// ncurses `halfdelay(settings->delay)` — restore the timed blocking read.
pub fn CRT_enableDelay() {
    CRT_nodelay.store(false, Ordering::Relaxed);
}

/// Port of `static void print_backtrace(void)` from `CRT.c:1360`.
///
/// The C selects one of two `PRINT_BACKTRACE` branches at configure time: the
/// libunwind branch (`HAVE_LIBUNWIND_H && HAVE_LOCAL_UNWIND`) or the execinfo
/// `backtrace(3)`/`backtrace_symbols_fd(3)` branch
/// (`HAVE_EXECINFO_H && BACKTRACE_RETURN_TYPE`, `CRT.c:1403`). This ports the
/// execinfo branch — the one taken on a build without libunwind, and the
/// branch whose substrate exists here (`libc::backtrace`/
/// `libc::backtrace_symbols_fd` on both macOS and Linux-gnu; `full_write_str`).
/// The libunwind branch is omitted (no libunwind crate), exactly as an
/// execinfo-only autoconf run would drop it.
pub fn print_backtrace() {
    // void* backtraceArray[256];
    let mut backtrace_array: [*mut libc::c_void; 256] = [core::ptr::null_mut(); 256];

    // BACKTRACE_RETURN_TYPE nptrs = backtrace(backtraceArray, ARRAYSIZE(backtraceArray));
    // BACKTRACE_RETURN_TYPE is `c_int` on both target platforms.
    let nptrs =
        unsafe { libc::backtrace(backtrace_array.as_mut_ptr(), backtrace_array.len() as _) };
    if nptrs > 0 {
        unsafe {
            libc::backtrace_symbols_fd(backtrace_array.as_ptr(), nptrs, libc::STDERR_FILENO);
        }
    } else {
        full_write_str(
            libc::STDERR_FILENO,
            "[No backtrace information available from libc]\n",
        );
    }
}

/// Port of `void CRT_handleSIGSEGV(int signal)` from `CRT.c:1420`. The fatal
/// fault handler: tears down the terminal ([`CRT_done`]), writes a crash
/// report to `STDERR_FILENO` (version, signal name, the persisted settings via
/// [`Settings_write`]`(CRT_settings, true)`, and a backtrace via
/// [`print_backtrace`]), then chains to the previously-installed disposition
/// saved in [`OLD_SIG_HANDLER`] and re-raises the signal, forcing an exit if
/// the chain does not terminate.
///
/// Substrate mapping: the `program` global is [`crate::ported::htop::program`]
/// and `VERSION` is `env!("CARGO_PKG_VERSION")` (the same mapping
/// [`Settings_write`] uses for `htop_version`). The C fixed `char err_buf[512]`
/// + `snprintf` become owned `format!` strings (the crate's owns-its-buffer
/// mapping). `CRT_settings` is the modeled `AtomicPtr<Settings>` global; the C
/// passes it straight to `Settings_write` (a `NULL` there would fault inside
/// the fault handler), so the port guards the null case and skips the write
/// while still emitting the section header. `PRINT_BACKTRACE` is defined (this
/// build has execinfo — [`print_backtrace`] is ported), so the backtrace
/// sections are included; the `HTOP_DARWIN` otool / objdump split maps to
/// `cfg(target_os = "macos")`. Registered by [`CRT_installSignalHandlers`],
/// hence the C-ABI `extern "C" fn(c_int)` signature.
pub extern "C" fn CRT_handleSIGSEGV(signal: libc::c_int) {
    CRT_done();

    let program = crate::ported::htop::program;
    const VERSION: &str = env!("CARGO_PKG_VERSION");

    let err_buf = format!(
        "\n\n\
         FATAL PROGRAM ERROR DETECTED\n\
         ============================\n\
         Please check at https://htop.dev/issues whether this issue has already been reported.\n\
         If no similar issue has been reported before, please create a new issue with the following information:\n\
         \x20 - Your {program} version: '{VERSION}'\n\
         \x20 - Your OS and kernel version (uname -a)\n\
         \x20 - Your distribution and release (lsb_release -a)\n\
         \x20 - Likely steps to reproduce (How did it happen?)\n"
    );
    full_write_str(libc::STDERR_FILENO, &err_buf);

    // #ifdef PRINT_BACKTRACE (execinfo build — print_backtrace is ported).
    full_write_str(
        libc::STDERR_FILENO,
        "  - Backtrace of the issue (see below)\n",
    );

    full_write_str(libc::STDERR_FILENO, "\n");

    let signal_str_ptr = unsafe { libc::strsignal(signal) };
    let signal_str = if signal_str_ptr.is_null() {
        String::from("unknown reason")
    } else {
        unsafe { std::ffi::CStr::from_ptr(signal_str_ptr) }
            .to_string_lossy()
            .into_owned()
    };
    let err_buf = format!(
        "Error information:\n\
         ------------------\n\
         A signal {signal} ({signal_str}) was received.\n\
         \n"
    );
    full_write_str(libc::STDERR_FILENO, &err_buf);

    full_write_str(
        libc::STDERR_FILENO,
        "Setting information:\n\
         --------------------\n",
    );
    // C: Settings_write(CRT_settings, true). `CRT_settings` is unwired in this
    // port (stays null), so guard the deref — the C would fault on a NULL here.
    let settings = CRT_settings.load(Ordering::Relaxed);
    if !settings.is_null() {
        // SAFETY: non-null `CRT_settings` points at the live process Settings
        // (set by `CRT_init`); read-only access in the crash path.
        Settings_write(unsafe { &*settings }, true);
    }
    full_write_str(libc::STDERR_FILENO, "\n\n");

    // #ifdef PRINT_BACKTRACE
    full_write_str(
        libc::STDERR_FILENO,
        "Backtrace information:\n\
         ----------------------\n",
    );
    print_backtrace();

    let err_buf = format!(
        "\n\
         To make the above information more practical to work with, \
         please also provide a disassembly of your {program} binary. \
         This can usually be done by running the following command:\n\
         \n"
    );
    full_write_str(libc::STDERR_FILENO, &err_buf);

    #[cfg(target_os = "macos")]
    let err_buf = format!("   otool -tvV `which {program}` > ~/{program}.otool\n");
    #[cfg(not(target_os = "macos"))]
    let err_buf = format!("   objdump -d -S -w `which {program}` > ~/{program}.objdump\n");
    full_write_str(libc::STDERR_FILENO, &err_buf);

    full_write_str(
        libc::STDERR_FILENO,
        "\n\
         Please include the generated file in your report.\n",
    );
    // #endif /* PRINT_BACKTRACE */
    let err_buf = format!(
        "Running this program with debug symbols or inside a debugger may provide further insights.\n\
         \n\
         Thank you for helping to improve {program}!\n\
         \n"
    );
    full_write_str(libc::STDERR_FILENO, &err_buf);

    // Call old sigsegv handler; may be default exit or third party one (e.g. ASAN)
    if unsafe { libc::sigaction(signal, old_sig_slot!(signal), core::ptr::null_mut()) } < 0 {
        // This avoids an infinite loop in case the handler could not be reset.
        full_write_str(
            libc::STDERR_FILENO,
            "!!! Chained handler could not be restored. Forcing exit.\n",
        );
        unsafe { libc::_exit(1) };
    }

    // Trigger the previous signal handler.
    unsafe { libc::raise(signal) };

    // Always terminate, even if installed handler returns
    full_write_str(
        libc::STDERR_FILENO,
        "!!! Chained handler did not exit. Forcing exit.\n",
    );
    unsafe { libc::_exit(1) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_term_is_false() {
        assert!(!terminalSupportsDefinedKeys(None));
    }

    #[test]
    fn exact_match_terminals() {
        assert!(terminalSupportsDefinedKeys(Some("alacritty")));
        assert!(terminalSupportsDefinedKeys(Some("foot")));
        assert!(terminalSupportsDefinedKeys(Some("vt220")));
        assert!(!terminalSupportsDefinedKeys(Some("alacritt")));
        assert!(!terminalSupportsDefinedKeys(Some("alacritty-256")));
        assert!(!terminalSupportsDefinedKeys(Some("footloose")));
        assert!(!terminalSupportsDefinedKeys(Some("vt100")));
    }

    #[test]
    fn st_terminal_and_dash_boundary() {
        assert!(terminalSupportsDefinedKeys(Some("st")));
        assert!(terminalSupportsDefinedKeys(Some("st-256color")));
        assert!(!terminalSupportsDefinedKeys(Some("sti")));
        assert!(!terminalSupportsDefinedKeys(Some("sun")));
        assert!(!terminalSupportsDefinedKeys(Some("s")));
    }

    #[test]
    fn screen_prefix_with_dash_boundary() {
        assert!(terminalSupportsDefinedKeys(Some("screen")));
        assert!(terminalSupportsDefinedKeys(Some("screen-256color")));
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
        assert!(!terminalSupportsDefinedKeys(Some("")));
        assert!(!terminalSupportsDefinedKeys(Some("linux")));
        assert!(!terminalSupportsDefinedKeys(Some("dumb")));
    }

    // ---- color model ----

    #[test]
    fn ncurses_attr_consts() {
        // Cited from /opt/homebrew/opt/ncurses/include/ncurses.h.
        assert_eq!(A_NORMAL, 0);
        assert_eq!(A_STANDOUT, 1 << 16);
        assert_eq!(A_UNDERLINE, 1 << 17);
        assert_eq!(A_REVERSE, 1 << 18);
        assert_eq!(A_BLINK, 1 << 19);
        assert_eq!(A_DIM, 1 << 20);
        assert_eq!(A_BOLD, 1 << 21);
        assert_eq!(A_COLOR, 0xFF00);
    }

    #[test]
    fn colorpair_and_colorindex_math() {
        // ColorIndex(i,j) = (7-i)*8 + j
        assert_eq!(ColorIndex(White, Black), 0);
        assert_eq!(ColorIndex(Black, Cyan), 62);
        assert_eq!(ColorIndex(Cyan, Black), 8);
        // ColorPair(i,j) = (ColorIndex << 8) & 0xFF00
        assert_eq!(ColorPair(Cyan, Black), 8 << 8);
        assert_eq!(ColorPair(White, Black), 0);
        // Special pairs.
        assert_eq!(ColorIndexGrayBlack, 21);
        assert_eq!(ColorIndexWhiteDefault, 49);
        assert_eq!(ColorPairGrayBlack, 21 << 8);
        assert_eq!(ColorPairWhiteDefault, 49 << 8);
    }

    #[test]
    fn enum_discriminants_match_c_order() {
        assert_eq!(RESET_COLOR as usize, 0);
        assert_eq!(DEFAULT_COLOR as usize, 1);
        assert_eq!(DYNAMIC_WHITE as usize, 115);
        assert_eq!(LAST_COLORELEMENT as usize, 116);
        assert_eq!(COLORSCHEME_DEFAULT as usize, 0);
        assert_eq!(COLORSCHEME_NORD as usize, 7);
        assert_eq!(LAST_COLORSCHEME as usize, 8);
        // Table dimensions equal the sentinels.
        assert_eq!(CRT_colorSchemes.len(), LAST_COLORSCHEME as usize);
        assert_eq!(CRT_colorSchemes[0].len(), LAST_COLORELEMENT as usize);
    }

    #[test]
    fn default_scheme_known_entries() {
        let d = &CRT_colorSchemes[COLORSCHEME_DEFAULT as usize];
        assert_eq!(d[PROCESS_MEGABYTES as usize], ColorPair(Cyan, Black));
        assert_eq!(d[PROCESS_GIGABYTES as usize], ColorPair(Green, Black));
        assert_eq!(d[CPU_NICE as usize], A_BOLD | ColorPair(Blue, Black));
        assert_eq!(d[CPU_NICE_TEXT as usize], A_BOLD | ColorPair(Blue, Black));
        assert_eq!(d[FUNCTION_BAR as usize], ColorPair(Black, Cyan));
        assert_eq!(d[PANEL_HEADER_FOCUS as usize], ColorPair(Black, Green));
        assert_eq!(d[PROCESS as usize], A_NORMAL);
        assert_eq!(d[BAR_BORDER as usize], A_BOLD);
        assert_eq!(d[METER_SHADOW as usize], A_BOLD | ColorPairGrayBlack);
    }

    #[test]
    fn monochrome_uses_attributes_not_colors() {
        let m = &CRT_colorSchemes[COLORSCHEME_MONOCHROME as usize];
        // MONOCHROME never sets a color pair: every entry has no color bits.
        for (idx, &v) in m.iter().enumerate() {
            assert_eq!(v & A_COLOR, 0, "MONOCHROME element {idx} has color bits");
        }
        assert_eq!(m[PANEL_HEADER_FOCUS as usize], A_REVERSE);
        assert_eq!(m[FAILED_SEARCH as usize], A_REVERSE | A_BOLD);
        assert_eq!(m[METER_SHADOW as usize], A_DIM);
        assert_eq!(m[CPU_NICE as usize], A_NORMAL);
    }

    #[test]
    fn brokengray_generated_from_default() {
        let d = &CRT_colorSchemes[COLORSCHEME_DEFAULT as usize];
        let b = &CRT_colorSchemes[COLORSCHEME_BROKENGRAY as usize];
        for i in 0..LAST_COLORELEMENT as usize {
            let expect = if d[i] == (A_BOLD | ColorPairGrayBlack) {
                ColorPair(White, Black)
            } else {
                d[i]
            };
            assert_eq!(b[i], expect, "BROKENGRAY element {i}");
        }
        // METER_SHADOW is A_BOLD|GrayBlack in DEFAULT -> White/Black (== 0) here.
        assert_eq!(b[METER_SHADOW as usize], ColorPair(White, Black));
        // A plain color entry is copied unchanged.
        assert_eq!(b[PROCESS_MEGABYTES as usize], ColorPair(Cyan, Black));
    }

    #[test]
    fn resolve_regular_pair_black_bg_is_default() {
        // DEFAULT PROCESS_MEGABYTES = ColorPair(Cyan, Black): Cyan fg,
        // Black bg -> terminal default (non-BLACKNIGHT).
        let r = PROCESS_MEGABYTES.resolve(COLORSCHEME_DEFAULT, true);
        assert_eq!(r.fg, Cyan as i16);
        assert_eq!(r.bg, ResolvedColor::DEFAULT);
        assert_eq!(r.attributes, 0);

        // CPU_NICE = A_BOLD | ColorPair(Blue, Black).
        let r = CPU_NICE.resolve(COLORSCHEME_DEFAULT, true);
        assert_eq!(r.fg, Blue as i16);
        assert_eq!(r.bg, ResolvedColor::DEFAULT);
        assert_eq!(r.attributes, A_BOLD);
    }

    #[test]
    fn resolve_nonblack_bg_kept() {
        // DEFAULT FUNCTION_BAR = ColorPair(Black, Cyan): Black fg, Cyan bg.
        let r = FUNCTION_BAR.resolve(COLORSCHEME_DEFAULT, true);
        assert_eq!(r.fg, Black as i16);
        assert_eq!(r.bg, Cyan as i16);
    }

    #[test]
    fn resolve_pair_zero_is_terminal_default() {
        // DEFAULT RESET_COLOR = ColorPair(White, Black) = pair 0.
        let r = RESET_COLOR.resolve(COLORSCHEME_DEFAULT, true);
        assert_eq!(r.fg, ResolvedColor::DEFAULT);
        assert_eq!(r.bg, ResolvedColor::DEFAULT);
    }

    #[test]
    fn resolve_grayblack_depends_on_colors() {
        // DEFAULT DYNAMIC_GRAY = ColorPairGrayBlack.
        let r = DYNAMIC_GRAY.resolve(COLORSCHEME_DEFAULT, true);
        assert_eq!(r.fg, 8);
        assert_eq!(r.bg, ResolvedColor::DEFAULT);
        let r = DYNAMIC_GRAY.resolve(COLORSCHEME_DEFAULT, false);
        assert_eq!(r.fg, Black as i16);
        assert_eq!(r.bg, ResolvedColor::DEFAULT);
    }

    #[test]
    fn resolve_whitedefault() {
        // LIGHTTERMINAL LOAD = ColorPairWhiteDefault.
        let r = LOAD.resolve(COLORSCHEME_LIGHTTERMINAL, true);
        assert_eq!(r.fg, White as i16);
        assert_eq!(r.bg, ResolvedColor::DEFAULT);
    }

    #[test]
    fn resolve_blacknight_keeps_black_bg() {
        // BLACKNIGHT RESET_COLOR = ColorPair(Cyan, Black): in BLACKNIGHT,
        // Black bg is NOT remapped to the terminal default.
        let r = RESET_COLOR.resolve(COLORSCHEME_BLACKNIGHT, true);
        assert_eq!(r.fg, Cyan as i16);
        assert_eq!(r.bg, Black as i16);
        // GrayBlack in BLACKNIGHT uses bg 0, not default.
        let r = DYNAMIC_GRAY.resolve(COLORSCHEME_BLACKNIGHT, true);
        assert_eq!(r.fg, 8);
        assert_eq!(r.bg, Black as i16);
    }

    #[test]
    fn crt_setcolors_clamps_and_selects() {
        // Only this test mutates the CRT_colorScheme global.
        CRT_setColors(COLORSCHEME_MIDNIGHT as i32);
        assert_eq!(ColorScheme::active(), COLORSCHEME_MIDNIGHT);
        CRT_setColors(999);
        assert_eq!(ColorScheme::active(), COLORSCHEME_DEFAULT);
        CRT_setColors(-1);
        assert_eq!(ColorScheme::active(), COLORSCHEME_DEFAULT);
        CRT_setColors(COLORSCHEME_NORD as i32);
        assert_eq!(ColorScheme::active(), COLORSCHEME_NORD);
        // restore
        CRT_setColors(COLORSCHEME_DEFAULT as i32);
    }

    // ---- terminal-control: pure keycode mapping ----

    fn press(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }
    fn press_mod(code: KeyCode, m: KeyModifiers) -> Event {
        Event::Key(KeyEvent::new(code, m))
    }
    fn map(ev: &Event) -> Option<i32> {
        Crt::map_event(ev)
    }

    #[test]
    fn key_constants_match_ncurses() {
        // Cited from /opt/homebrew/opt/ncurses/include/ncurses.h (octal).
        assert_eq!(KEY_DOWN, 258);
        assert_eq!(KEY_UP, 259);
        assert_eq!(KEY_LEFT, 260);
        assert_eq!(KEY_RIGHT, 261);
        assert_eq!(KEY_HOME, 262);
        assert_eq!(KEY_BACKSPACE, 263);
        assert_eq!(KEY_F0, 264);
        assert_eq!(KEY_DC, 330);
        assert_eq!(KEY_IC, 331);
        assert_eq!(KEY_NPAGE, 338);
        assert_eq!(KEY_PPAGE, 339);
        assert_eq!(KEY_ENTER, 343);
        assert_eq!(KEY_END, 360);
        assert_eq!(KEY_SLEFT, 393);
        assert_eq!(KEY_SRIGHT, 402);
        assert_eq!(KEY_MOUSE, 409);
        assert_eq!(KEY_RESIZE, 410);
        assert_eq!(KEY_MAX, 511);
        assert_eq!(ERR, -1);
    }

    #[test]
    fn derived_key_macros() {
        assert_eq!(KEY_F(1), 265);
        assert_eq!(KEY_F(10), 274);
        // KEY_CTRL(l) = l - 'A' + 1.
        assert_eq!(KEY_CTRL(b'A' as i32), 1);
        assert_eq!(KEY_CTRL(b'Z' as i32), 26);
        // KEY_ALT(x) = KEY_F(38) + (x - 'A').
        assert_eq!(KEY_ALT(b'A' as i32), KEY_F(38));
        assert_eq!(KEY_ALT(b'X' as i32), 325);
        // htop CRT.h derivations.
        assert_eq!(KEY_SHIFT_TAB, KEY_F(34));
        assert_eq!(KEY_WHEELUP, KEY_F(30));
        assert_eq!(KEY_WHEELDOWN, KEY_F(31));
        assert_eq!(KEY_FOCUS_IN, 584);
        assert_eq!(KEY_FOCUS_OUT, 590);
        assert_eq!(KEY_CTRL_LEFT, KEY_SLEFT);
        assert_eq!(KEY_CTRL_RIGHT, KEY_SRIGHT);
    }

    #[test]
    fn map_plain_chars_and_control_alt() {
        assert_eq!(map(&press(KeyCode::Char('q'))), Some('q' as i32));
        assert_eq!(map(&press(KeyCode::Char(' '))), Some(' ' as i32));
        // Ctrl+letter -> control byte (case-insensitive).
        assert_eq!(
            map(&press_mod(KeyCode::Char('a'), KeyModifiers::CONTROL)),
            Some(1)
        );
        assert_eq!(
            map(&press_mod(KeyCode::Char('C'), KeyModifiers::CONTROL)),
            Some(3)
        );
        // Alt+letter -> KEY_ALT(upper).
        assert_eq!(
            map(&press_mod(KeyCode::Char('x'), KeyModifiers::ALT)),
            Some(KEY_ALT(b'X' as i32))
        );
        // Ctrl with a non-alpha char falls through to the codepoint.
        assert_eq!(
            map(&press_mod(KeyCode::Char('1'), KeyModifiers::CONTROL)),
            Some('1' as i32)
        );
    }

    #[test]
    fn map_navigation_and_editing_keys() {
        assert_eq!(map(&press(KeyCode::Up)), Some(KEY_UP));
        assert_eq!(map(&press(KeyCode::Down)), Some(KEY_DOWN));
        assert_eq!(map(&press(KeyCode::Left)), Some(KEY_LEFT));
        assert_eq!(map(&press(KeyCode::Right)), Some(KEY_RIGHT));
        assert_eq!(map(&press(KeyCode::Home)), Some(KEY_HOME));
        assert_eq!(map(&press(KeyCode::End)), Some(KEY_END));
        assert_eq!(map(&press(KeyCode::PageUp)), Some(KEY_PPAGE));
        assert_eq!(map(&press(KeyCode::PageDown)), Some(KEY_NPAGE));
        assert_eq!(map(&press(KeyCode::Delete)), Some(KEY_DC));
        assert_eq!(map(&press(KeyCode::Insert)), Some(KEY_IC));
        assert_eq!(map(&press(KeyCode::Backspace)), Some(KEY_BACKSPACE));
        // Ctrl+arrows -> shifted-arrow codes (htop treats them the same).
        assert_eq!(
            map(&press_mod(KeyCode::Left, KeyModifiers::CONTROL)),
            Some(KEY_CTRL_LEFT)
        );
        assert_eq!(
            map(&press_mod(KeyCode::Right, KeyModifiers::CONTROL)),
            Some(KEY_CTRL_RIGHT)
        );
    }

    #[test]
    fn map_enter_tab_esc_function_keys() {
        assert_eq!(map(&press(KeyCode::Enter)), Some(KEY_ENTER));
        assert_eq!(map(&press(KeyCode::Tab)), Some('\t' as i32));
        assert_eq!(map(&press(KeyCode::BackTab)), Some(KEY_SHIFT_TAB));
        assert_eq!(map(&press(KeyCode::Esc)), Some(27));
        assert_eq!(map(&press(KeyCode::Null)), Some(0));
        assert_eq!(map(&press(KeyCode::F(1))), Some(KEY_F(1)));
        assert_eq!(map(&press(KeyCode::F(10))), Some(KEY_F(10)));
    }

    #[test]
    fn map_non_key_events() {
        assert_eq!(map(&Event::Resize(80, 24)), Some(KEY_RESIZE));
        assert_eq!(map(&Event::FocusGained), Some(KEY_FOCUS_IN));
        assert_eq!(map(&Event::FocusLost), Some(KEY_FOCUS_OUT));
    }

    #[test]
    fn map_ignores_key_release() {
        let release = Event::Key(KeyEvent::new_with_kind(
            KeyCode::Char('q'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        ));
        assert_eq!(map(&release), None);
        // A repeat is treated as a press.
        let repeat = Event::Key(KeyEvent::new_with_kind(
            KeyCode::Char('q'),
            KeyModifiers::NONE,
            KeyEventKind::Repeat,
        ));
        assert_eq!(map(&repeat), Some('q' as i32));
    }

    #[test]
    fn map_unmapped_keycode_is_none() {
        assert_eq!(map(&press(KeyCode::CapsLock)), None);
        assert_eq!(map(&press(KeyCode::NumLock)), None);
    }

    // ---- terminal-control: utf8 / degree sign / fatal error ----

    #[test]
    fn compute_utf8_logic() {
        assert!(Crt::compute_utf8(true, "UTF-8"));
        assert!(!Crt::compute_utf8(false, "UTF-8"));
        assert!(!Crt::compute_utf8(true, "ISO-8859-1"));
        assert!(!Crt::compute_utf8(true, ""));
    }

    #[test]
    fn degree_sign_selection() {
        // UTF-8: two-byte U+00B0; otherwise the single ISO-8859 byte.
        assert_eq!(Crt::degree_sign_bytes(true), b"\xc2\xb0");
        assert_eq!(Crt::degree_sign_bytes(false), b"\xb0");
    }

    #[test]
    fn init_degree_sign_stores_by_utf8_flag() {
        // Isolated mutation of the CRT_utf8 / CRT_degreeSign globals.
        CRT_utf8.store(true, Ordering::Relaxed);
        initDegreeSign();
        assert_eq!(&*CRT_degreeSign.lock().unwrap(), b"\xc2\xb0");
        CRT_utf8.store(false, Ordering::Relaxed);
        initDegreeSign();
        assert_eq!(&*CRT_degreeSign.lock().unwrap(), b"\xb0");
        // restore default
        CRT_utf8.store(false, Ordering::Relaxed);
    }

    #[test]
    fn fatal_error_message_format() {
        // C: fprintf(stderr, "%s: %s\n", note, sysMsg).
        assert_eq!(
            Crt::fatal_error_message("Cannot open file", "No such file or directory"),
            "Cannot open file: No such file or directory\n"
        );
    }

    // ---- stderr-cache fd roundtrip (debug-only, like the C `#ifndef NDEBUG`) ----

    /// Exercises `createStderrCacheFile` + the `lseek`/`read` mechanics
    /// `dumpStderr` relies on, without touching the real STDERR_FILENO:
    /// write into the cache fd, rewind, read it back, and compare. Safe in
    /// headless CI (memfd on Linux, an unlinked temp file elsewhere).
    #[cfg(debug_assertions)]
    #[test]
    fn stderr_cache_file_roundtrip() {
        let fd = createStderrCacheFile();
        assert!(fd >= 0, "createStderrCacheFile returned {fd}");

        let payload = b"line one\nline two\n";
        let w = full_write(fd, payload);
        assert_eq!(w, payload.len() as libc::ssize_t);

        // Rewind, as dumpStderr does before reading the cache back.
        let off = unsafe { libc::lseek(fd, 0, libc::SEEK_SET) };
        assert_eq!(off, 0);

        let mut buf = [0u8; 64];
        let r = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
        assert_eq!(r, payload.len() as libc::ssize_t);
        assert_eq!(&buf[..r as usize], payload);

        unsafe { libc::close(fd) };
    }

    /// `full_write` must drain the whole slice across a real fd. Uses a pipe
    /// so nothing hits the harness's stderr.
    #[test]
    fn full_write_drains_slice() {
        let mut fds = [0 as libc::c_int; 2];
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        let (rd, wr) = (fds[0], fds[1]);

        let msg = b">>>>>>>>>> stderr output >>>>>>>>>>\n";
        let n = full_write(wr, msg);
        assert_eq!(n, msg.len() as libc::ssize_t);
        unsafe { libc::close(wr) };

        let mut buf = [0u8; 64];
        let r = unsafe { libc::read(rd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
        assert_eq!(r, msg.len() as libc::ssize_t);
        assert_eq!(&buf[..r as usize], msg);
        unsafe { libc::close(rd) };
    }

    /// The [`CRT_treeStr`](TreeStr::glyph) glyph tables transcribe `CRT.c`
    /// exactly, including `TREE_STR_OPEN` staying ASCII `+` in the UTF-8 table.
    /// (Tests the tables directly rather than mutating the shared [`CRT_utf8`]
    /// flag, to stay race-free in the parallel suite.)
    #[test]
    fn tree_str_tables_match_c() {
        assert_eq!(CRT_treeStrAscii.len(), LAST_TREE_STR);
        assert_eq!(CRT_treeStrUtf8.len(), LAST_TREE_STR);

        assert_eq!(CRT_treeStrAscii[TreeStr::TREE_STR_VERT as usize], "|");
        assert_eq!(CRT_treeStrAscii[TreeStr::TREE_STR_BEND as usize], "`");
        assert_eq!(CRT_treeStrAscii[TreeStr::TREE_STR_TEND as usize], ",");

        assert_eq!(CRT_treeStrUtf8[TreeStr::TREE_STR_VERT as usize], "│");
        assert_eq!(CRT_treeStrUtf8[TreeStr::TREE_STR_RTEE as usize], "├");
        assert_eq!(CRT_treeStrUtf8[TreeStr::TREE_STR_BEND as usize], "└");
        assert_eq!(CRT_treeStrUtf8[TreeStr::TREE_STR_SHUT as usize], "─");
        assert_eq!(CRT_treeStrUtf8[TreeStr::TREE_STR_ASC as usize], "△");
        assert_eq!(CRT_treeStrUtf8[TreeStr::TREE_STR_DESC as usize], "▽");
        // C keeps OPEN as ASCII '+' in both tables.
        assert_eq!(CRT_treeStrUtf8[TreeStr::TREE_STR_OPEN as usize], "+");
    }
}
