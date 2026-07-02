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
//! Still stubbed (`todo!()`): signal handlers (SIGSEGV/SIGTERM backtrace),
//! stderr redirect/dump, `CRT_debug_impl`, and the signal-handler
//! install/reset — debugging infrastructure that is out of scope here.
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
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

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

/// Port of `void CRT_setColors(int colorScheme)` from `CRT.c:1334` —
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

/// `#define KEY_CTRL(l) ((l)-'A'+1)` (Panel.h:88).
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
    /// CRT.c:1265 — `CRT_utf8 = allowUnicode && String_eq(nl_langinfo(CODESET), "UTF-8")`.
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

    /// CRT.c:1311 — `fprintf(stderr, "%s: %s\n", note, sysMsg)`. Factored
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
/// parser already recognizes those escape sequences), the signal-handler
/// install (`CRT_installSignalHandlers`, still stubbed), and the
/// `CRT_treeStr` selection (needs the not-yet-ported `TREE_STR` tables).
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

    // CRT.c:1265  allowUnicode && nl_langinfo(CODESET) == "UTF-8"
    let utf8 = Crt::compute_utf8(allow_unicode, &Crt::current_codeset());
    CRT_utf8.store(utf8, Ordering::Relaxed);

    CRT_setMouse(enable_mouse);

    initDegreeSign();
}

/// Port of `void CRT_done(void)` from `CRT.c:1290`.
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

/// Port of `void CRT_fatalError(const char* note)` from `CRT.c:1308`.
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

/// Port of `int CRT_readKey(void)` from `CRT.c:1315`.
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

/// Port of `void CRT_disableDelay(void)` from `CRT.c:1324`.
/// ncurses `nodelay(stdscr, TRUE)` — make input non-blocking.
pub fn CRT_disableDelay() {
    CRT_nodelay.store(true, Ordering::Relaxed);
}

/// Port of `void CRT_enableDelay(void)` from `CRT.c:1330`.
/// ncurses `halfdelay(settings->delay)` — restore the timed blocking read.
pub fn CRT_enableDelay() {
    CRT_nodelay.store(false, Ordering::Relaxed);
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
}
