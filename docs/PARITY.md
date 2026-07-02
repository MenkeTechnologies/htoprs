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

## Structure

- `tests/parity/harness.rs` — shared helper: locate the binaries, run both,
  canonicalize the rebrand, assert stdout + exit-code equality.
- `tests/parity/cli_parity.rs` — the CLI cases.
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

**7 passing, 0 failing, 2 documented gaps (ignored).**

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
