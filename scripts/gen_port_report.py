#!/usr/bin/env python3
"""Regenerate docs/port_report.html — the htoprs port-progress report.

Walks the htop C source (the spec) and the Rust port, then reports
per-file and overall coverage. Numbers are derived from source at run
time — nothing is hardcoded.

Definition of "ported": a `pub fn <name>` under src/ported/ whose name
matches a function *defined* in the htop C source. The C-side count is
definitions only (not every referenced libc symbol), so coverage is
meaningful.

Usage:
    HTOP_C_SOURCE=~/forkedRepos/htop scripts/gen_port_report.py
    # defaults to ~/forkedRepos/htop
"""
from __future__ import annotations
import html
import json
import os
import re
import sys
from collections import defaultdict
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
HTOP_SRC = Path(os.environ.get("HTOP_C_SOURCE", str(Path.home() / "forkedRepos" / "htop")))
PORTED = ROOT / "src" / "ported"
OUT = ROOT / "docs" / "port_report.html"

C_KEYWORDS = {
    "if", "for", "while", "switch", "return", "else", "do", "sizeof",
    "static", "extern", "struct", "union", "enum", "typedef", "const",
    "volatile", "inline", "register", "auto", "goto", "break", "continue",
    "case", "default",
}

# A C function *definition* line: starts at column 0, has an identifier
# immediately before `(`. We then require an opening brace within a few
# lines and reject prototypes (a `;` before any `{`).
RE_C_DEF = re.compile(r"^[A-Za-z_][\w\s\*]*?\b([A-Za-z_]\w*)\s*\(")


def walk_c_defs() -> dict[str, list[tuple[str, int]]]:
    """C file stem/name -> [(rel_path, line)] of function definitions."""
    idx: dict[str, list[tuple[str, int]]] = defaultdict(list)
    for c in sorted(HTOP_SRC.rglob("*.c")):
        rel = c.relative_to(HTOP_SRC).as_posix()
        try:
            lines = c.read_text(errors="replace").splitlines()
        except OSError:
            continue
        for i, line in enumerate(lines, 1):
            if not line or line[0].isspace() or line[0] in "#/*}":
                continue
            m = RE_C_DEF.match(line)
            if not m:
                continue
            name = m.group(1)
            if name in C_KEYWORDS:
                continue
            tail = " ".join(lines[i - 1:i + 5])
            brace = tail.find("{")
            semi = tail.find(";")
            if brace == -1:
                continue
            if semi != -1 and semi < brace:
                continue  # prototype, not a definition
            idx[name].append((rel, i))
    return idx


RE_RS_FN = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:unsafe\s+)?(?:async\s+)?fn\s+([A-Za-z_]\w*)")
RE_PORT_CITE = re.compile(r"Port of .*?`?([A-Za-z_][\w]*\.c):(\d+)`?")


def walk_ported() -> dict[str, dict]:
    """ported fn name -> {file, line, cite_file, cite_line}."""
    out: dict[str, dict] = {}
    pending_cite = None
    for rs in sorted(PORTED.rglob("*.rs")):
        rel = rs.relative_to(ROOT).as_posix()
        pending_cite = None
        for i, line in enumerate(rs.read_text(errors="replace").splitlines(), 1):
            cite = RE_PORT_CITE.search(line)
            if cite:
                pending_cite = (cite.group(1), int(cite.group(2)))
            m = RE_RS_FN.match(line)
            if m and line[: len(line) - len(line.lstrip())] == "":
                name = m.group(1)
                out[name] = {
                    "rs_file": rel,
                    "rs_line": i,
                    "cite_file": pending_cite[0] if pending_cite else None,
                    "cite_line": pending_cite[1] if pending_cite else None,
                }
                pending_cite = None
    return out


def main() -> int:
    if not HTOP_SRC.is_dir():
        print(f"ERROR: htop source not found at {HTOP_SRC}", file=sys.stderr)
        print("Set HTOP_C_SOURCE to override.", file=sys.stderr)
        return 1

    c_defs = walk_c_defs()
    ported = walk_ported()

    # Per-C-file coverage: how many of a file's defined fns are ported.
    by_cfile: dict[str, dict] = {}
    for name, locs in c_defs.items():
        for (rel, _line) in locs:
            by_cfile.setdefault(rel, {"total": set(), "ported": set()})
            by_cfile[rel]["total"].add(name)

    ported_names = set(ported)
    for rel, d in by_cfile.items():
        d["ported"] = {n for n in d["total"] if n in ported_names}

    total_c = len(c_defs)
    total_ported = len({n for n in ported_names if n in c_defs})

    rows = []
    for rel in sorted(by_cfile):
        d = by_cfile[rel]
        t = len(d["total"])
        p = len(d["ported"])
        if p == 0:
            continue
        rows.append((rel, p, t))

    ts = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    pct = (100.0 * total_ported / total_c) if total_c else 0.0

    data = {
        "generated": ts,
        "htop_source": str(HTOP_SRC),
        "c_functions_defined": total_c,
        "ported": total_ported,
        "coverage_pct": round(pct, 2),
        "files_started": len(rows),
        "per_file": [
            {"cfile": rel, "ported": p, "defined": t} for (rel, p, t) in rows
        ],
    }

    OUT.parent.mkdir(parents=True, exist_ok=True)
    body_rows = "\n".join(
        f"<tr><td>{html.escape(rel)}</td><td>{p}</td><td>{t}</td>"
        f"<td>{100.0 * p / t:.0f}%</td></tr>"
        for (rel, p, t) in rows
    )
    doc = f"""<!DOCTYPE html>
<!--PORT-REPORT-SCHEMA: c_functions_defined=htop C fn definitions; ported=ported fns matching a C name; per_file=[cfile,ported,defined]-->
<html lang="en"><head><meta charset="utf-8">
<title>htoprs port report</title>
<style>
 body {{ background:#0b0e14; color:#c8d3f5; font:14px/1.5 ui-monospace,monospace; margin:2rem; }}
 h1 {{ color:#7aa2f7; }}
 .stat {{ font-size:2rem; color:#9ece6a; }}
 table {{ border-collapse:collapse; margin-top:1rem; }}
 th,td {{ border:1px solid #2a2f45; padding:.3rem .8rem; text-align:left; }}
 th {{ color:#7aa2f7; }}
 .muted {{ color:#565f89; }}
</style></head><body>
<h1>htoprs — port report</h1>
<p class="muted">Spec: htop C source at {html.escape(str(HTOP_SRC))} · generated {ts}</p>
<p><span class="stat">{total_ported}</span> / {total_c} C functions ported
 (<b>{pct:.2f}%</b>) across {len(rows)} file(s) started.</p>
<table>
<thead><tr><th>C file</th><th>ported</th><th>defined</th><th>coverage</th></tr></thead>
<tbody>
{body_rows}
</tbody></table>
<script id="port-report-data" type="application/json">
{json.dumps(data, indent=2)}
</script>
</body></html>
"""
    OUT.write_text(doc)
    print(f"Wrote {OUT.relative_to(ROOT)}")
    print(f"  {total_ported}/{total_c} C fns ported ({pct:.2f}%), "
          f"{len(rows)} file(s) started")
    return 0


if __name__ == "__main__":
    sys.exit(main())
