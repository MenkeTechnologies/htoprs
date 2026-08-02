# htoprs parity suite

htoprs is a from-source port of htop, so the definition of "correct" is **htop
itself** — specifically htop **3.5.1**, the version the port tracks (C sources
under `~/forkedRepos/htop`). The suite runs the same inputs through the
reference `htop` binary and `htoprs` and compares output byte-for-byte, modulo
the deliberate rebrand (program name `htoprs`→`htop`, version banner). This
mirrors the sibling ports' parity harnesses (zshrs vs `zsh`, ztmux vs `tmux`).

Version matters: htop's help text and flag set change across minor releases, so
the harness only runs when the reference is the ported **3.5** series and skips
otherwise (a different-version htop would produce false divergences). This is
the same version-pinning caution the ztmux suite documents.

## Running

```sh
cargo test --test parity                 # run the whole suite
cargo test --test parity cli             # filter to the CLI cases
cargo test --test parity xutils          # filter to the C-reference cases
HTOP_REF=/path/to/htop cargo test --test parity   # pin the reference binary
```

The reference is found via `HTOP_REF`, then `/opt/homebrew/bin/htop`,
`/usr/local/bin/htop`, `/usr/bin/htop`, `/bin/htop`. When none matches htop
3.5.x, every comparison is a no-op (green), so CI without htop passes while a
dev box with htop runs the real diff.

## Two comparison surfaces

1. **CLI** (`cli_parity.rs`) — run the `htoprs` and `htop` binaries on the same
   args and diff stdout+exit (modulo rebrand). Covers `-V`/`--version`, the
   unknown-flag getopt error, and `--sort-key=help`. `-h`/`--help` is a
   *deliberate* divergence, not a parity surface: `main.rs` routes it to the
   branded `extensions::help` printer, so the `help` cases pin the divergence
   (branded output present, and it differs from htop) instead of asserting
   equality. The faithful plain port lives on in `commandline::printHelpFlag`.
2. **Library functions** (`xutils_parity.rs`) — the per-function surface. A tiny
   C reference harness (`cref/htop_cref.c`) is compiled against htop's
   **genuine `XUtils.c`** and invoked per input; the Rust port is
   called with the same input and the outputs are compared byte-for-byte. This
   is the zshrs/ztmux "reference vs port" model applied to functions, so the
   reference is htop's real C code — not a reimplementation. Requires the htop C
   source (`HTOP_C_SOURCE`, default `~/forkedRepos/htop`) and a C compiler;
   skips otherwise.

   The harness must compile the C in the **same config branch the Rust port
   targeted** — e.g. htoprs ports htop's `!HAVE_BUILTIN_CTZ` `countTrailingZeros`
   fallback (mod-37 table), so `cref/config.h` deliberately leaves
   `HAVE_BUILTIN_CTZ` undefined; otherwise the builtin path diverges at `x==0`
   (where `__builtin_ctz` is undefined).

## Structure

- `tests/parity/harness.rs` — shared CLI helper: locate the binaries, run both,
  canonicalize the rebrand, assert stdout + exit-code equality.
- `tests/parity/cli_parity.rs` — the CLI cases.
- `tests/parity/xutils_parity.rs` — the C-reference library-function cases.
- `tests/parity/cref/{htop_cref.c,config.h}` — the C reference harness compiled
  against htop's `XUtils.c`.
- `tests/parity/main.rs` — the single aggregated `[[test]]` target (Cargo does
  not auto-discover files under a `tests/` subdir). Add a parity area by
  dropping a `*_parity.rs` file here and adding one `mod` line.

## Status

The interactive TUI is not a deterministic diff surface, so the parity cases are
the non-interactive ones: the `CommandLine.c` `parseArguments` getopt_long
behaviors, the version printer, and the `XUtils.c` function layer checked
against the real C.

| Case | Status |
| --- | --- |
| `htoprs --help` / `-h` is branded and diverges from `htop --help` | **pass** (intentional divergence, pinned) |
| `htoprs --version` / `-V` vs `htop --version` | **pass** |
| `-h` == `--help`, `-V` == `--version` (self-consistency) | **pass** |
| version banner format (`<name> <semver>`) | **pass** |
| unknown flag: getopt error, empty stdout, exit 1 | **pass** |
| `--sort-key=help` column listing | **pass** |
| `XUtils.c`: `countDigits` (22 inputs: bases 2/8/10/16, 0, SIZE_MAX) | **pass** vs C |
| `XUtils.c`: `countTrailingZeros` (46 inputs incl. `x==0`, every bit) | **pass** vs C |
| `XUtils.c`: `compareRealNumbers` (equal / </> / ±0 / 1e±300) | **pass** vs C |
| `XUtils.c`: `sumPositiveValues` (empty / all-neg / mixed; bit-exact f64) | **pass** vs C |
| `XUtils.c`: `String_cat` / `String_trim` (empty, whitespace, UTF-8) | **pass** vs C |
| `XUtils.c`: `String_contains_i` (case, multi flag, empty) | **pass** vs C |
| `XUtils.c`: `String_split` / `String_splitFirst` (leading/trailing/empty sep) | **pass** vs C |

**18 passing, 0 failing, 0 ignored** (`cargo test --test parity`). The library
cases run each ported `XUtils.c` function across many edge inputs against the
real C. Add the next module (e.g. `String_startsWith`/`String_eq`, `Hashtable`,
`Vector`) by extending `htop_cref.c`'s dispatch and adding a `*_parity.rs` case.

Per the endgame rule, a newly-ported CLI surface adds its case here first —
confirm it fails against the reference, then fix the port until it passes. As
more of the non-interactive CLI is exercised (`--pid`, `--user`, `--tree`
validation, config/rc handling), add the case and its row to the table above.
