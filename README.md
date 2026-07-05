```
██╗  ██╗████████╗ ██████╗ ██████╗ ██████╗ ███████╗
██║  ██║╚══██╔══╝██╔═══██╗██╔══██╗██╔══██╗██╔════╝
███████║   ██║   ██║   ██║██████╔╝██████╔╝███████╗
██╔══██║   ██║   ██║   ██║██╔═══╝ ██╔══██╗╚════██║
██║  ██║   ██║   ╚██████╔╝██║     ██║  ██║███████║
╚═╝  ╚═╝   ╚═╝    ╚═════╝ ╚═╝     ╚═╝  ╚═╝╚══════╝
```

![Rust](https://img.shields.io/badge/Rust-2021-05d9e8?style=flat-square)
![htop](https://img.shields.io/badge/htop-3.5.1_port-ff2a6d?style=flat-square)
![status](https://img.shields.io/badge/status-porting-f9e900?style=flat-square)
![MenkeTechnologies](https://img.shields.io/badge/MenkeTechnologies-stack-d300c5?style=flat-square)

### `[THE FAITHFUL RUST PORT OF HTOP]`

> *"The htop C source is the spec — ported function-for-function, not reimagined."*

**htoprs** is a faithful Rust port of [htop](https://github.com/htop-dev/htop), the
interactive process viewer. Every function under `src/ported/` ports a specific htop
C function, cited by `<File>.c:<line>` in its doc comment. Created by MenkeTechnologies.

### [`Read the Docs`](https://menketechnologies.github.io/htoprs/) &middot; [`Engineering Report`](https://menketechnologies.github.io/htoprs/report.html) · [`Port Report`](https://menketechnologies.github.io/htoprs/port_report.html)

---

## Porting methodology

The C source is the specification. Ports are faithful — the original C is
translated function-for-function, never reimplemented from scratch. This is
enforced mechanically, following the same precedent as `zshrs`.

- **Spec:** htop **3.5.1**, 131 `.c` files — vendored in-repo as the
  `vendor/htop` submodule (pinned to the 3.5.1 tag) and mirrored at the
  developer checkout `~/forkedRepos/htop`. `docs/port_report.html` is generated
  against `vendor/htop`; the snapshot/report tools take `$HTOP_C_SOURCE`
  (default `~/forkedRepos/htop`) to point at either.
- **Port tree:** `src/ported/<file>.rs` — one Rust module per C file. Each `fn`
  carries a `/// Port of` citation naming its `<File>.c:<line>` origin.
- **Extensions tree:** `src/extensions/<name>.rs` — htoprs-original code that is
  not a translation of htop C and is therefore exempt from the port-purity gate.
  `extensions::theme` holds the named color-scheme system (31 built-in 6-color
  palettes plus custom-theme plumbing), and `extensions::overlay` the themed
  keyboard-help overlay, theme chooser, and theme editor (rendering into a
  `ratatui::Buffer`) — both ported from iftoprs. `extensions::colors` makes a
  chosen theme recolor the live htop UI in 256-color via a base16-style ANSI
  palette remap consulted at the single `Ncurses::to_color` choke point, and
  `extensions::prefs` persists the selection to `~/.config/htoprs/prefs.json`.
  The overlay is wired into `ScreenManager_run`: `z` opens the theme (color
  scheme) chooser, `~` the editor, `h`/`?`/F1 the themed help overlay (`Esc`
  closes it), `g` toggles the header, `B` toggles the border. These are all
  keys htop leaves free — lowercase `c` (tag process + children) and `C`
  (setup) belong to htop, so the overlay must not shadow them; likewise `b` is
  the bar fill-style cycler below and `x` is htop's file-locks screen. Toggles
  and the bar-style change surface a transient status toast
  (`overlay::draw_status`, ported from iftoprs).
  `extensions::bridge` materializes the live ported `Process` rows as the
  `Proc` model (via `Object::as_process`), and `extensions::panels` is the
  running-TUI wiring for the htoprs-original monitoring capabilities — the
  monitoring analog of the theme overlay. A thread-local state is fed the real
  table each sample tick (advancing the per-PID history rings, the debounced
  threshold alerts, and the CPU history graph) and gets first refusal on keys,
  with hotkeys chosen from those htop leaves unbound:

  | Key | Capability |
  |-----|------------|
  | `f` | Fuzzy process finder (Enter jumps the cursor to the match) |
  | `r` | Regex / substring filter over comm/cmdline/user, with a saved named store (`~/.config/htoprs/filters.json`) |
  | `d` | Snapshot: first press captures a baseline, next press diffs the live table against it (`+`started `-`exited `~`changed); `w` writes the snapshot JSON |
  | `o` | Export the current table to JSON + CSV under `~/.config/htoprs/` |
  | `A` | Threshold alerts — the rule set and every currently-firing PID |
  | `G` | Braille CPU history graph (system total plus the selected PID) |
  | `y` | Aggregate/pivot: live CPU+memory totals grouped by user / command / parent (`Tab` cycles the key) |
  | `:` | Command palette — fuzzy-search every action by name and run it (reuses the `f` matcher); reaches both extension and htop actions |
  | `v` | Cycle the per-PID CPU sparkline: off → narrow right-edge column → CPU-scaled inline braille graph |

  Two of these reach the rows themselves rather than a modal, injected at the
  per-row draw site in `Panel_draw` (the same extension-hook pattern the theme
  border uses, so no new ported `fn`): a firing-alert PID's row is recolored,
  and the `v` sparkline is drawn on the rows. `v` cycles three states — off, a
  narrow braille sparkline overdrawn at each row's right edge, and an inline
  graph mode where each process grows a full-width braille CPU graph beneath its
  info line. The graph is rendered by the same braille canvas as the `G` history
  graph, and its **height scales with the process's CPU**: idle processes stay a
  single line, busy ones grow up to `SPARK_GRAPH_H` graph lines (more CPU = more
  rows). This makes the process panel variable-height: `Panel_draw`/`Panel_onKey`
  project every screen-Y, page step, and scroll clamp through per-row heights
  (`item_height` = `1 + graph_lines(cpu)` for a process, `1` otherwise), so the
  cursor, paging, and scrolling all track whole processes. Non-process panels
  keep `rowHeight = 1` and are byte-identical to the ported behavior.
- **Bar fill-glyph cycler (`extensions::barstyle`, ported from storageshower):**
  `b` cycles the character every bar meter (CPU, Memory, Swap, …) fills with,
  through five styles — Classic (`|`, htop's default), Gradient (position-shaded
  `█▓▒░` with a `▸` tip), Solid (`█`), Thin (`▬`/`▸`), and Ascii (`#`/`>`) —
  keeping each segment's semantic color. Each press shows the iftoprs-style
  status toast (`overlay::draw_status`, e.g. `Bar style: solid`) centered near
  the bottom for 3s. The selection persists to `~/.config/htoprs/prefs.json` and
  is restored on launch. It is consulted by the ported `BarMeterMode_draw` fill
  loop (`barstyle::fill_glyph`, `None` ⇒ htop's native glyph) and wired into the
  keybinding table (`Action_setBindings` binds the `keys['b']` slot htop leaves
  free unless `HAVE_BACKTRACE_SCREEN` is set) as an `Htop_Action`.
- **Port-purity gate (`build.rs`):** on every `cargo build` / `cargo test` /
  `cargo check` that touches `src/ported/`, every free `fn` name is checked
  against the htop C-function snapshot at `tests/data/htop_c_fn_names.txt`. A
  `fn` whose name has no C counterpart fails the build. Rust-original helpers
  are rejected; genuine architectural helpers must be justified in
  `tests/data/fake_fn_allowlist.txt`. The gate cannot be bypassed by
  `cargo test --test X`.
- **C-name snapshot:** regenerate with `tests/data/extract_c_fn_names.sh` after
  pulling upstream htop (`HTOP_C_SOURCE=~/forkedRepos/htop`).
- **Stub scaffold (`scripts/gen_port_stubs.py`):** lays out the full port
  surface — one module per C file, one `todo!()` stub `pub fn` per C function
  (named to satisfy the gate). Stubs are placeholders, not ports; replace each
  with a faithful body and a `/// Port of` cite. Already-ported modules are
  never overwritten.
- **Port report:** `scripts/gen_port_report.py` walks the C source and the Rust
  port and writes `docs/port_report.html` with per-file and overall coverage,
  derived from source at run time (nothing hardcoded). A `todo!()` /
  `unimplemented!()` body is counted as **stubbed**, never **ported**, so
  scaffolding cannot inflate the coverage number.

## Current state

The port covers **915 of 1093 C functions (83.7%)** across **121 of the 131 C
files**, with 13 stubs remaining — the TUI runs as a daily driver on macOS. The
core is ported end-to-end: the process model and table build (`Process`,
`ProcessTable`, `Table`, `Row`, `Machine`), the container/util layer (`Vector`,
`Hashtable`, `RichString`, `XUtils`), the full meter set (CPU, Memory, Swap,
Load, Battery, Network, DiskIO, GPU, ZFS, and the dynamic meters), the UI panels
and main loop (`Panel`, `ScreenManager`, `MainPanel`, `FunctionBar`, `Header`,
and the setup/columns/colors/display-options screens), key dispatch (`Action`,
`IncSet`, `LineEditor`), the `CRT` terminal layer, and the per-OS machine /
process-table backends (darwin / linux / freebsd / netbsd / openbsd /
dragonflybsd / solaris). Functions that need genuinely unportable C substrate
(the `xMalloc` family, raw `Object**`/bucket-table internals, varargs
formatters) stay intentionally unported; a shrinking set of honest `todo!()`
stubs marks work still in flight. Overall and per-file coverage — real ports vs
stubs — lives in `docs/port_report.html` (derived from source at run time —
nothing hardcoded).

On top of the port sits an `src/extensions/` layer (18 modules, exempt from the
port-purity gate) — the named color-theme system, the help/theme overlay, and
the live monitoring suite (per-PID history, alerts, braille CPU graphs, finder,
diffs, exporters) described above.

### Terminal backend & substrate

htoprs must render **byte-for-byte identical to htop** (enforced by the parity
suite). The terminal layer is [crossterm](https://crates.io/crates/crossterm) —
pure-Rust, vendorable, cross-arch, no C dependency — giving full control over
every glyph/color/attribute so the output matches htop while the draw code is a
behavioral (not line-for-line) port. The substrate the UI renders through is
ported: `Object.c` (htop's vtable OOP → a Rust `Object` trait with a class-chain
`Object_isA`), `RichString.c` (the full styled-character buffer), and `CRT.c`'s
**color model** — the `ColorElements` enum and every `CRT_colorSchemes` entry
transcribed verbatim so colors match htop exactly. The terminal-control fns
(`CRT_init`/`readKey`, `Panel`/`ScreenManager` draw) and the platform
data-collection layer (`Platform_*`, process scan) are now ported and drive the
live TUI; the remaining gaps are the un-started files tracked in the port report.

**`XUtils.c`** — string / math utilities:

| C function | notes |
|---|---|
| `String_cat` | concatenation |
| `String_trim` | trims leading/trailing space, tab, newline (only those three) |
| `String_split` | splits on a separator; interior empties kept, trailing empty dropped |
| `String_splitFirst` | splits on first separator only |
| `String_contains_i` | case-insensitive substring; `\|`-multi-needle mode |
| `compareRealNumbers` | NaN-aware ordering (NaN sorts first) |
| `sumPositiveValues` | sum of strictly-positive values, NaN skipped |
| `countDigits` | digit count in a given base, with overflow guard |
| `countTrailingZeros` | mod-37 lowest-set-bit table |

**`Vector.c`** — container sort / search core, ported as generics over a slice
with a C-`int`-returning comparator (the `Object**` pointer array becomes a
slice; signed `isize` indices preserve the C's below-`left` arithmetic):

| C function | notes |
|---|---|
| `swap` | exchange two slots |
| `partition` | Lomuto partition, pivot moved to `right` |
| `quickSort` | recursive quicksort, pivot `left + (right - left) / 2` |
| `insertionSort` | in-place insertion sort over `[left, right]` |
| `Vector_indexOf` | linear search, C `-1` sentinel preserved |

**`Hashtable.c`** — prime-table math:

| C function | notes |
|---|---|
| `nextPrime` | smallest OEIS prime `>= n`; aborts (panics) if none fits |

**`Meter.c`** — value formatting:

| C function | notes |
|---|---|
| `Meter_humanUnit` | kibibytes → human-readable string (`K`…`Q`, `inf` cap) |

The C allocation wrappers, null-terminated-string helpers, varargs formatters,
and `String_freeArray` (XUtils.c); the `Object**` dynamic-array memory
machinery — `Vector_new` / `_insert` / `_add` / `_resizeIfNecessary` and the
rest (Vector.c); and the open-addressing bucket table — `Hashtable_new` /
`_put` / `_get` / `_foreach` and the rest (Hashtable.c) — have no faithful
safe-Rust analog (Rust's `Vec` / `HashMap` own allocation, bounds, probing,
and lifetimes) and are intentionally not ported. `combSort` is commented-out
dead code in `Vector.c` and is not ported.

## Build & test

```sh
cargo build          # runs the port-purity gate
cargo test           # ports have hand-crafted unit tests pinning C edge behavior
cargo test --test parity   # diff htoprs output vs the reference htop 3.5.x
python3 scripts/gen_port_report.py   # regenerate docs/port_report.html
```

The **parity suite** (`tests/parity/`) runs the same inputs through the
reference `htop` (the C original htoprs is ported from) and `htoprs`, then
compares output byte-for-byte, modulo the deliberate rebrand
(`htoprs`→`htop`, version banner). It skips gracefully when no matching htop
3.5.x is installed, so CI stays green; a dev box with htop runs the real
comparison. Point it at a specific reference with `HTOP_REF=/path/to/htop`.
See [docs/PARITY.md](docs/PARITY.md).
