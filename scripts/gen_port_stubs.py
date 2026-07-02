#!/usr/bin/env python3
"""Scaffold `src/ported/<stem>.rs` stub modules for htop C files that
have no Rust module yet.

For every top-level htop `.c` file (the port tree mirrors the C root),
this writes one Rust module containing a `todo!()` stub `pub fn` for
each function *defined* in that C file whose name is also present in the
build.rs snapshot (`tests/data/htop_c_fn_names.txt`) — so every stub
passes the port-purity gate and corresponds to a real C function.

The stubs are honest placeholders, NOT ports:
  - the body is `todo!("port of <Stem>.c:<line>")`
  - the doc comment reads `/// TODO: port ...` (lowercase "port" — it
    deliberately does NOT match `gen_port_report.py`'s `Port of` cite
    regex, and `gen_port_report.py` classifies `todo!()`/`unimplemented!()`
    bodies as *stubbed*, not *ported*, so the coverage number is not
    inflated by scaffolding).

Files that already have a Rust module (fully or partially ported) are
left untouched — this only fills in the missing ones. Re-runnable:
existing modules are never overwritten.

Usage:
    HTOP_C_SOURCE=~/forkedRepos/htop scripts/gen_port_stubs.py [--dry-run]
"""
from __future__ import annotations
import os
import re
import sys
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
HTOP_SRC = Path(os.environ.get("HTOP_C_SOURCE", str(Path.home() / "forkedRepos" / "htop")))
PORTED = ROOT / "src" / "ported"
SNAPSHOT = ROOT / "tests" / "data" / "htop_c_fn_names.txt"

C_KEYWORDS = {
    "if", "for", "while", "switch", "return", "else", "do", "sizeof",
    "static", "extern", "struct", "union", "enum", "typedef", "const",
    "volatile", "inline", "register", "auto", "goto", "break", "continue",
    "case", "default",
}

# C function names that are Rust keywords can't be a `pub fn` without a
# raw identifier (which the build.rs gate can't parse); skip them —
# they get ported by hand with `r#name` if they ever occur.
RUST_KEYWORDS = {
    "as", "break", "const", "continue", "crate", "dyn", "else", "enum",
    "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop",
    "match", "mod", "move", "mut", "pub", "ref", "return", "self", "Self",
    "static", "struct", "super", "trait", "true", "type", "unsafe", "use",
    "where", "while", "async", "await", "abstract", "become", "box", "do",
    "final", "macro", "override", "priv", "typeof", "unsized", "virtual",
    "yield", "try", "gen",
}

RE_VALID_IDENT = re.compile(r"^[a-z_][a-z0-9_]*$")

# Same C-definition matcher gen_port_report.py uses: identifier at
# column 0 immediately before `(`, an opening brace within a few lines,
# and no `;` before that brace (which would make it a prototype).
RE_C_DEF = re.compile(r"^[A-Za-z_][\w\s\*]*?\b([A-Za-z_]\w*)\s*\(")


def load_snapshot_names() -> set[str]:
    names: set[str] = set()
    for line in SNAPSHOT.read_text(errors="replace").splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        _, _, fn = line.partition(":")
        if fn:
            names.add(fn)
    return names


def c_defs_by_stem(srcdir: Path) -> dict[str, list[tuple[str, int, str]]]:
    """C file stem -> ordered [(fn_name, line, signature_text)] for `srcdir`
    (non-recursive; platform subdirs are scaffolded one at a time so their
    colliding stems, e.g. linux/Platform.c vs darwin/Platform.c, land in
    separate mirrored module dirs)."""
    out: dict[str, list[tuple[str, int, str]]] = defaultdict(list)
    for c in sorted(srcdir.glob("*.c")):
        stem = c.stem.lower()
        if not RE_VALID_IDENT.match(stem):
            continue  # e.g. `pcp-htop` — not a valid Rust module name
        lines = c.read_text(errors="replace").splitlines()
        seen: set[str] = set()
        for i, line in enumerate(lines, 1):
            if not line or line[0].isspace() or line[0] in "#/*}":
                continue
            m = RE_C_DEF.match(line)
            if not m:
                continue
            name = m.group(1)
            if name in C_KEYWORDS or name in RUST_KEYWORDS or name in seen:
                continue
            tail = " ".join(lines[i - 1:i + 5])
            brace = tail.find("{")
            semi = tail.find(";")
            if brace == -1 or (semi != -1 and semi < brace):
                continue
            sig = line.split("{")[0].strip().rstrip(")").strip()
            if len(sig) > 160:
                sig = sig[:157] + "..."
            out[stem].append((name, i, sig))
            seen.add(name)
    return out


MODULE_HEADER = """\
//! Stub scaffold for `{cfile}` — NOT yet ported.
//!
//! Every `pub fn` below is a placeholder (`todo!()`) named after a real
//! htop C function so the port-purity gate accepts the module and the
//! port surface is laid out. Replace each stub with a faithful port of
//! the C body, updating the signature and the doc comment to `Port of
//! `{cfile}`:<line>.` as you go. `gen_port_report.py` counts these
//! `todo!()` bodies as *stubbed*, not *ported*, so scaffolding does not
//! inflate coverage.
#![allow(non_snake_case)]
#![allow(dead_code)]
"""

STUB_FN = """\
/// TODO: port of `{sig}` from `{cfile}:{line}`.
pub fn {name}() {{
    todo!("port of {cfile}:{line}")
}}
"""


def _subdir_arg() -> str | None:
    for i, a in enumerate(sys.argv):
        if a == "--subdir" and i + 1 < len(sys.argv):
            return sys.argv[i + 1]
    return None


def main() -> int:
    dry = "--dry-run" in sys.argv
    if not HTOP_SRC.is_dir():
        print(f"ERROR: htop source not found at {HTOP_SRC}", file=sys.stderr)
        return 1
    if not SNAPSHOT.is_file():
        print(f"ERROR: snapshot not found at {SNAPSHOT}", file=sys.stderr)
        return 1

    subdir = _subdir_arg()
    # srcdir = the C directory to scan; outdir = the mirrored ported dir;
    # modrs = the mod.rs that lists the generated modules.
    if subdir:
        if not RE_VALID_IDENT.match(subdir):
            print(f"ERROR: --subdir {subdir!r} is not a valid Rust module name", file=sys.stderr)
            return 1
        srcdir = HTOP_SRC / subdir
        outdir = PORTED / subdir
        modrs = outdir / "mod.rs"
        if not srcdir.is_dir():
            print(f"ERROR: {srcdir} not found", file=sys.stderr)
            return 1
    else:
        srcdir = HTOP_SRC
        outdir = PORTED
        modrs = PORTED / "mod.rs"

    snapshot = load_snapshot_names()
    defs = c_defs_by_stem(srcdir)

    existing = {p.stem for p in outdir.glob("*.rs")} if outdir.is_dir() else set()
    existing.discard("mod")
    created: list[tuple[str, int]] = []

    for stem, fns in sorted(defs.items()):
        if stem in existing:
            continue  # already has a module (real or partial port) — leave it
        cfile = next(f.name for f in srcdir.glob("*.c") if f.stem.lower() == stem)
        gated = [(n, ln, sig) for (n, ln, sig) in fns if n in snapshot]
        if not gated:
            continue
        body = [MODULE_HEADER.format(cfile=cfile), ""]
        for (name, line, sig) in gated:
            body.append(STUB_FN.format(name=name, line=line, sig=sig.replace("`", "'"), cfile=cfile))
        text = "\n".join(body).rstrip() + "\n"
        created.append((stem, len(gated)))
        if not dry:
            outdir.mkdir(parents=True, exist_ok=True)
            (outdir / f"{stem}.rs").write_text(text)

    # Register the generated modules in the target mod.rs without
    # disturbing existing lines (the tree may be edited concurrently).
    header = "//! Ported htop platform modules.\n\n" if subdir else None
    src = modrs.read_text() if modrs.exists() else (header or "")
    have = set(re.findall(r"^pub mod (\w+);", src, re.M))
    want = sorted(have | {stem for stem, _ in created})
    if want != sorted(have):
        prefix = src.split("pub mod ")[0].rstrip()
        prefix = (prefix + "\n\n") if prefix else (header or "")
        new_src = prefix + "".join(f"pub mod {m};\n" for m in want)
        if not dry:
            outdir.mkdir(parents=True, exist_ok=True)
            modrs.write_text(new_src)

    # When scaffolding a subdir, make sure the parent mod.rs declares it.
    if subdir:
        parent = PORTED / "mod.rs"
        psrc = parent.read_text()
        if not re.search(rf"^pub mod {subdir};", psrc, re.M):
            if not dry:
                parent.write_text(psrc.rstrip() + f"\npub mod {subdir};\n")

    total_fns = sum(n for _, n in created)
    verb = "would create" if dry else "created"
    where = f"{subdir}/" if subdir else ""
    print(f"{verb} {len(created)} stub module(s), {total_fns} stub fn(s) under {where or 'src/ported/'}")
    for stem, n in created:
        print(f"  {where}{stem}.rs  ({n} stubs)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
