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

- **Spec:** htop **3.5.1** at `~/forkedRepos/htop` (131 `.c` files).
- **Port tree:** `src/ported/<file>.rs` — one Rust module per C file. Each `fn`
  carries a `/// Port of` citation naming its `<File>.c:<line>` origin.
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

The pure-logic layers are ported — string/math utilities, the container
sort/search algorithms, the prime table, and the human-readable unit
formatter (detailed below). Partial ports also cover the faithfully-portable
subset of eight more files: `Process.c` (cmdline/comm string parsing, process
state char), `LineEditor.c` (text-buffer editing and cursor motion),
`OptionItem.c` (`CheckItem`/`NumberItem` accessors and editing),
`RichString.c` (`RichString_findChar`), `ListItem.c` (`ListItem_compare`),
`Affinity.c` (`Affinity_add`), `History.c` (the command-history ring), and
`Row.c` (`Row_printPercentage`). Functions that need still-unported substrate
(ncurses/`RichString` drawing, `CRT` colors, `Panel`, `Object` vtables,
syscalls) remain honest `todo!()` stubs; the rest of the C source is likewise
scaffolded with stubs so the full surface is laid out. Overall and per-file
coverage — real ports vs stubs — lives in `docs/port_report.html` (derived
from source at run time — nothing hardcoded).

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
python3 scripts/gen_port_report.py   # regenerate docs/port_report.html
```
