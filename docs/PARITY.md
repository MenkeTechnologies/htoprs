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
cargo test --test parity -- --ignored    # documented not-yet-ported gaps
HTOP_REF=/path/to/htop cargo test --test parity   # pin the reference binary
```

The reference is found via `HTOP_REF`, then `/opt/homebrew/bin/htop`,
`/usr/local/bin/htop`, `/usr/bin/htop`, `/bin/htop`. When none matches htop
3.5.x, every comparison is a no-op (green), so CI without htop passes while a
dev box with htop runs the real diff.

## Two comparison surfaces

1. **CLI** (`cli_parity.rs`) — run the `htoprs` and `htop` binaries on the same
   args and diff stdout+exit (modulo rebrand). Covers the wired `-V`/`-h`.
2. **Library functions** (`xutils_parity.rs`) — the richer surface for an
   early-stage port. A tiny C reference harness (`cref/htop_cref.c`) is compiled
   against htop's **genuine `XUtils.c`** and invoked per input; the Rust port is
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

htoprs is an early-stage port: only the `CommandLine.c` `-V`/`--version` and
`-h`/`--help` printers are wired, so those are today's deterministic surfaces.

| Case | Status |
| --- | --- |
| `htoprs --help` / `-h` vs `htop --help` | **pass** (byte-identical after rebrand) |
| `htoprs --version` / `-V` vs `htop --version` | **pass** |
| `-h` == `--help`, `-V` == `--version` (self-consistency) | **pass** |
| version banner format (`<name> <semver>`) | **pass** |
| `XUtils.c`: `countDigits` (22 inputs: bases 2/8/10/16, 0, SIZE_MAX) | **pass** vs C |
| `XUtils.c`: `countTrailingZeros` (46 inputs incl. `x==0`, every bit) | **pass** vs C |
| `XUtils.c`: `compareRealNumbers` (equal / </> / ±0 / 1e±300) | **pass** vs C |
| `XUtils.c`: `sumPositiveValues` (empty / all-neg / mixed; bit-exact f64) | **pass** vs C |
| `XUtils.c`: `String_cat` / `String_trim` (empty, whitespace, UTF-8) | **pass** vs C |
| `XUtils.c`: `String_contains_i` (case, multi flag, empty) | **pass** vs C |
| `XUtils.c`: `String_split` / `String_splitFirst` (leading/trailing/empty sep) | **pass** vs C |

**16 passing, 0 failing, 2 documented gaps (ignored).** The library cases run
each ported `XUtils.c` function across many edge inputs against the real C.
Add the next module (e.g. `String_startsWith`/`String_eq`, `Hashtable`,
`Vector`) by extending `htop_cref.c`'s dispatch and adding a `*_parity.rs` case.

### Documented gaps (`--ignored`)

These are the exact htop behaviors htoprs must reproduce once the relevant port
lands; each is pinned as an `#[ignore]` case that goes green when the port
does. Per the endgame rule, a newly-ported CLI surface adds its case here
first — confirm it fails, then fix the port until it passes.

- **`unknown_flag_getopt_error`** — htop treats an unknown flag as a getopt
  error (usage to stderr, exit 1); htoprs prints a placeholder line. Needs the
  `CommandLine_parseArgs` getopt_long switch.
- **`sort_key_help_listing`** — `--sort-key=help` lists the sortable columns and
  exits 0; needs the ProcessTable/columns port.

As the non-interactive CLI is ported (argument parsing, `--pid`, `--user`,
`--tree` validation, `--sort-key=help`, config/rc handling), add cases here and
move them from the gaps section to the status table.
