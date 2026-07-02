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
//! Still stubbed (`todo!()`): every terminal-control fn (`CRT_init`,
//! `CRT_done`, `CRT_readKey`, `CRT_setMouse`, signal handlers,
//! `CRT_fatalError`, stderr redirect, backtrace) — they need crossterm
//! terminal setup and belong to a later phase.
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

use std::sync::atomic::{AtomicUsize, Ordering};

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
    t[COLORSCHEME_DEFAULT as usize][PROCESS_THREAD_BASENAME as usize] = A_BOLD | ColorPair(Green, Black);
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
    t[COLORSCHEME_DEFAULT as usize][PRESSURE_STALL_SIXTY as usize] = A_BOLD | ColorPair(Cyan, Black);
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
    t[COLORSCHEME_BLACKONWHITE as usize][PANEL_SELECTION_FOLLOW as usize] = ColorPair(Black, Yellow);
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
    t[COLORSCHEME_BLACKONWHITE as usize][METER_VALUE_ERROR as usize] = A_BOLD | ColorPair(Red, White);
    t[COLORSCHEME_BLACKONWHITE as usize][METER_VALUE_IOREAD as usize] = ColorPair(Green, White);
    t[COLORSCHEME_BLACKONWHITE as usize][METER_VALUE_IOWRITE as usize] = ColorPair(Yellow, White);
    t[COLORSCHEME_BLACKONWHITE as usize][METER_VALUE_NOTICE as usize] = A_BOLD | ColorPair(Yellow, White);
    t[COLORSCHEME_BLACKONWHITE as usize][METER_VALUE_OK as usize] = ColorPair(Green, White);
    t[COLORSCHEME_BLACKONWHITE as usize][METER_VALUE_WARN as usize] = A_BOLD | ColorPair(Yellow, White);
    t[COLORSCHEME_BLACKONWHITE as usize][LED_COLOR as usize] = ColorPair(Green, White);
    t[COLORSCHEME_BLACKONWHITE as usize][TASKS_RUNNING as usize] = ColorPair(Green, White);
    t[COLORSCHEME_BLACKONWHITE as usize][PROCESS as usize] = ColorPair(Black, White);
    t[COLORSCHEME_BLACKONWHITE as usize][PROCESS_SHADOW as usize] = A_BOLD | ColorPair(Black, White);
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
    t[COLORSCHEME_BLACKONWHITE as usize][PROCESS_THREAD_BASENAME as usize] = A_BOLD | ColorPair(Blue, White);
    t[COLORSCHEME_BLACKONWHITE as usize][PROCESS_COMM as usize] = ColorPair(Magenta, White);
    t[COLORSCHEME_BLACKONWHITE as usize][PROCESS_THREAD_COMM as usize] = ColorPair(Green, White);
    t[COLORSCHEME_BLACKONWHITE as usize][PROCESS_PRIV as usize] = ColorPair(Magenta, White);
    t[COLORSCHEME_BLACKONWHITE as usize][BAR_BORDER as usize] = ColorPair(Blue, White);
    t[COLORSCHEME_BLACKONWHITE as usize][BAR_SHADOW as usize] = ColorPair(Black, White);
    t[COLORSCHEME_BLACKONWHITE as usize][SWAP as usize] = ColorPair(Red, White);
    t[COLORSCHEME_BLACKONWHITE as usize][SWAP_CACHE as usize] = ColorPair(Yellow, White);
    t[COLORSCHEME_BLACKONWHITE as usize][SWAP_FRONTSWAP as usize] = A_BOLD | ColorPair(Black, White);
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
    t[COLORSCHEME_BLACKONWHITE as usize][SCREENS_OTH_BORDER as usize] = A_BOLD | ColorPair(Black, White);
    t[COLORSCHEME_BLACKONWHITE as usize][SCREENS_OTH_TEXT as usize] = A_BOLD | ColorPair(Black, White);
    t[COLORSCHEME_BLACKONWHITE as usize][SCREENS_CUR_BORDER as usize] = ColorPair(Green, Green);
    t[COLORSCHEME_BLACKONWHITE as usize][SCREENS_CUR_TEXT as usize] = ColorPair(Black, Green);
    t[COLORSCHEME_BLACKONWHITE as usize][PRESSURE_STALL_THREEHUNDRED as usize] = ColorPair(Black, White);
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
    t[COLORSCHEME_BLACKONWHITE as usize][DYNAMIC_DARKGRAY as usize] = A_BOLD | ColorPair(Black, White);
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
    t[COLORSCHEME_LIGHTTERMINAL as usize][PANEL_SELECTION_FOLLOW as usize] = ColorPair(Black, Yellow);
    t[COLORSCHEME_LIGHTTERMINAL as usize][PANEL_SELECTION_UNFOCUS as usize] = ColorPair(Blue, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][FAILED_SEARCH as usize] = ColorPair(Red, Cyan);
    t[COLORSCHEME_LIGHTTERMINAL as usize][FAILED_READ as usize] = ColorPair(Red, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][PAUSED as usize] = A_BOLD | ColorPair(Yellow, Cyan);
    t[COLORSCHEME_LIGHTTERMINAL as usize][UPTIME as usize] = ColorPair(Yellow, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][BATTERY as usize] = ColorPair(Yellow, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][LARGE_NUMBER as usize] = ColorPair(Red, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][METER_SHADOW as usize] = A_BOLD | ColorPairGrayBlack;
    t[COLORSCHEME_LIGHTTERMINAL as usize][METER_TEXT as usize] = ColorPair(Blue, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][METER_VALUE as usize] = ColorPair(Black, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][METER_VALUE_ERROR as usize] = A_BOLD | ColorPair(Red, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][METER_VALUE_IOREAD as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][METER_VALUE_IOWRITE as usize] = ColorPair(Yellow, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][METER_VALUE_NOTICE as usize] = A_BOLD | ColorPairWhiteDefault;
    t[COLORSCHEME_LIGHTTERMINAL as usize][METER_VALUE_OK as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][METER_VALUE_WARN as usize] = A_BOLD | ColorPair(Yellow, Black);
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
    t[COLORSCHEME_LIGHTTERMINAL as usize][PROCESS_D_STATE as usize] = A_BOLD | ColorPair(Red, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][PROCESS_HIGH_PRIORITY as usize] = ColorPair(Red, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][PROCESS_LOW_PRIORITY as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][PROCESS_NEW as usize] = ColorPair(Black, Green);
    t[COLORSCHEME_LIGHTTERMINAL as usize][PROCESS_TOMB as usize] = ColorPair(Black, Red);
    t[COLORSCHEME_LIGHTTERMINAL as usize][PROCESS_THREAD as usize] = ColorPair(Blue, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][PROCESS_THREAD_BASENAME as usize] = A_BOLD | ColorPair(Blue, Black);
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
    t[COLORSCHEME_LIGHTTERMINAL as usize][PRESSURE_STALL_THREEHUNDRED as usize] = ColorPair(Black, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][PRESSURE_STALL_SIXTY as usize] = ColorPair(Black, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][PRESSURE_STALL_TEN as usize] = ColorPair(Black, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][FILE_DESCRIPTOR_USED as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_LIGHTTERMINAL as usize][FILE_DESCRIPTOR_MAX as usize] = A_BOLD | ColorPair(Blue, Black);
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
    t[COLORSCHEME_MIDNIGHT as usize][PANEL_SELECTION_UNFOCUS as usize] = A_BOLD | ColorPair(Yellow, Blue);
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
    t[COLORSCHEME_MIDNIGHT as usize][PROCESS_THREAD_BASENAME as usize] = A_BOLD | ColorPair(Green, Blue);
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
    t[COLORSCHEME_MIDNIGHT as usize][LOAD_AVERAGE_FIFTEEN as usize] = A_BOLD | ColorPair(Black, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][LOAD_AVERAGE_FIVE as usize] = A_NORMAL | ColorPair(White, Blue);
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
    t[COLORSCHEME_MIDNIGHT as usize][SCREENS_OTH_BORDER as usize] = A_BOLD | ColorPair(Yellow, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][SCREENS_OTH_TEXT as usize] = ColorPair(Cyan, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][SCREENS_CUR_BORDER as usize] = ColorPair(Cyan, Cyan);
    t[COLORSCHEME_MIDNIGHT as usize][SCREENS_CUR_TEXT as usize] = ColorPair(Black, Cyan);
    t[COLORSCHEME_MIDNIGHT as usize][PRESSURE_STALL_THREEHUNDRED as usize] = A_BOLD | ColorPair(Black, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][PRESSURE_STALL_SIXTY as usize] = A_NORMAL | ColorPair(White, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][PRESSURE_STALL_TEN as usize] = A_BOLD | ColorPair(White, Blue);
    t[COLORSCHEME_MIDNIGHT as usize][FILE_DESCRIPTOR_USED as usize] = A_BOLD | ColorPair(Green, Blue);
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
    t[COLORSCHEME_BLACKNIGHT as usize][METER_VALUE_NOTICE as usize] = A_BOLD | ColorPair(White, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][METER_VALUE_OK as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][METER_VALUE_WARN as usize] = A_BOLD | ColorPair(Yellow, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][LED_COLOR as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][TASKS_RUNNING as usize] = A_BOLD | ColorPair(Green, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][PROCESS as usize] = ColorPair(Cyan, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][PROCESS_SHADOW as usize] = A_BOLD | ColorPairGrayBlack;
    t[COLORSCHEME_BLACKNIGHT as usize][PROCESS_TAG as usize] = A_BOLD | ColorPair(Yellow, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][PROCESS_MEGABYTES as usize] = A_BOLD | ColorPair(Green, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][PROCESS_GIGABYTES as usize] = A_BOLD | ColorPair(Yellow, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][PROCESS_BASENAME as usize] = A_BOLD | ColorPair(Green, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][PROCESS_TREE as usize] = ColorPair(Cyan, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][PROCESS_THREAD as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][PROCESS_THREAD_BASENAME as usize] = A_BOLD | ColorPair(Blue, Black);
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
    t[COLORSCHEME_BLACKNIGHT as usize][LOAD_AVERAGE_ONE as usize] = A_BOLD | ColorPair(Green, Black);
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
    t[COLORSCHEME_BLACKNIGHT as usize][SCREENS_CUR_BORDER as usize] = A_BOLD | ColorPair(White, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][SCREENS_CUR_TEXT as usize] = A_BOLD | ColorPair(Green, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][PRESSURE_STALL_THREEHUNDRED as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][PRESSURE_STALL_SIXTY as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][PRESSURE_STALL_TEN as usize] = A_BOLD | ColorPair(Green, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][FILE_DESCRIPTOR_USED as usize] = ColorPair(Green, Black);
    t[COLORSCHEME_BLACKNIGHT as usize][FILE_DESCRIPTOR_MAX as usize] = A_BOLD | ColorPair(Blue, Black);
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
    t[COLORSCHEME_NORD as usize][FAILED_SEARCH as usize] = A_REVERSE | A_BOLD | ColorPair(Yellow, Black);
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
    t[COLORSCHEME_NORD as usize][PRESSURE_STALL_THREEHUNDRED as usize] = A_BOLD | ColorPairGrayBlack;
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
            return ResolvedColor { fg: Self::DEFAULT, bg: Self::DEFAULT, attributes };
        }
        if pair == ColorIndexGrayBlack {
            let fg = if colors_gt_8 { 8 } else { 0 };
            let bg = if blacknight { 0 } else { Self::DEFAULT };
            return ResolvedColor { fg, bg, attributes };
        }
        if pair == ColorIndexWhiteDefault {
            return ResolvedColor { fg: White as i16, bg: Self::DEFAULT, attributes };
        }
        // ColorIndex(i, j) = (7 - i) * 8 + j  =>  j = pair % 8, i = 7 - pair / 8.
        let j = pair & 7;
        let i = 7 - (pair >> 3);
        let bg = if !blacknight && j == Black { Self::DEFAULT } else { j as i16 };
        ResolvedColor { fg: i as i16, bg, attributes }
    }
}

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
}
