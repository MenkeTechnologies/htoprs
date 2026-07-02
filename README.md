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
- **Port report:** `scripts/gen_port_report.py` walks the C source and the Rust
  port and writes `docs/port_report.html` with per-file and overall coverage,
  derived from source at run time (nothing hardcoded).

## Current state

Ported so far — `XUtils.c` (string / math utilities):

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

The C allocation wrappers, null-terminated-string helpers, varargs formatters,
and `String_freeArray` have no faithful safe-Rust analog (Rust owns its
allocation, bounds, and lifetimes) and are intentionally not ported.

## Build & test

```sh
cargo build          # runs the port-purity gate
cargo test           # ports have hand-crafted unit tests pinning C edge behavior
python3 scripts/gen_port_report.py   # regenerate docs/port_report.html
```
