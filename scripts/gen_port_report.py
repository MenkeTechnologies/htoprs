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


# Matches a Rust fn definition. The `extern "<abi>"` group recognizes ported
# C-callback shims declared `pub extern "C" fn ...` (signal handlers, libproc
# walk callbacks); without it those real ports were miscounted as unported.
RE_RS_FN = re.compile(
    r'^\s*(?:pub(?:\([^)]*\))?\s+)?(?:unsafe\s+)?(?:async\s+)?(?:extern\s+"[^"]*"\s+)?fn\s+([A-Za-z_]\w*)'
)
RE_PORT_CITE = re.compile(r"Port of .*?`?([A-Za-z_][\w]*\.c):(\d+)`?")


def _body_is_stub(lines: list[str], fn_idx: int) -> bool:
    """True if the fn starting at `lines[fn_idx]` has a `todo!()` /
    `unimplemented!()` body — i.e. it's a scaffold stub, not a port.

    Brace-matches from the fn's first `{` to its close and looks for a
    stub macro in between. A genuine port never contains `todo!()`, so
    this cannot misclassify real work as a stub."""
    depth = 0
    started = False
    body: list[str] = []
    for line in lines[fn_idx:]:
        for ch in line:
            if ch == "{":
                depth += 1
                started = True
            elif ch == "}":
                depth -= 1
        if started:
            body.append(line)
            if depth <= 0:
                break
    text = "\n".join(body)
    return "todo!(" in text or "unimplemented!(" in text


def walk_ported() -> tuple[dict[str, dict], dict[str, dict]]:
    """(real, stubbed): fn name -> {rs_file, rs_line, cite_file, cite_line}.

    `real`    = a column-0 fn with an implemented body.
    `stubbed` = a column-0 fn whose body is `todo!()`/`unimplemented!()`.
    Stubs are scaffolding, not ports, so they are reported separately and
    never counted toward coverage."""
    real: dict[str, dict] = {}
    stubbed: dict[str, dict] = {}
    for rs in sorted(PORTED.rglob("*.rs")):
        rel = rs.relative_to(ROOT).as_posix()
        lines = rs.read_text(errors="replace").splitlines()
        pending_cite = None
        for i, line in enumerate(lines, 1):
            cite = RE_PORT_CITE.search(line)
            if cite:
                pending_cite = (cite.group(1), int(cite.group(2)))
            m = RE_RS_FN.match(line)
            if m and line[: len(line) - len(line.lstrip())] == "":
                name = m.group(1)
                entry = {
                    "rs_file": rel,
                    "rs_line": i,
                    "cite_file": pending_cite[0] if pending_cite else None,
                    "cite_line": pending_cite[1] if pending_cite else None,
                }
                (stubbed if _body_is_stub(lines, i - 1) else real)[name] = entry
                pending_cite = None
    return real, stubbed


def main() -> int:
    if not HTOP_SRC.is_dir():
        print(f"ERROR: htop source not found at {HTOP_SRC}", file=sys.stderr)
        print("Set HTOP_C_SOURCE to override.", file=sys.stderr)
        return 1

    c_defs = walk_c_defs()
    real, stubbed = walk_ported()

    # Per-C-file coverage: how many of a file's defined fns are ported
    # (real body) vs merely stubbed (todo!() scaffold).
    by_cfile: dict[str, dict] = {}
    for name, locs in c_defs.items():
        for (rel, _line) in locs:
            by_cfile.setdefault(rel, {"total": set(), "ported": set(), "stubbed": set()})
            by_cfile[rel]["total"].add(name)

    real_names = set(real)
    stub_names = set(stubbed)
    for rel, d in by_cfile.items():
        d["ported"] = {n for n in d["total"] if n in real_names}
        d["stubbed"] = {n for n in d["total"] if n in stub_names and n not in real_names}

    total_c = len(c_defs)
    total_ported = len({n for n in real_names if n in c_defs})
    total_stubbed = len({n for n in stub_names if n in c_defs and n not in real_names})

    rows = []
    for rel in sorted(by_cfile):
        d = by_cfile[rel]
        t = len(d["total"])
        p = len(d["ported"])
        s = len(d["stubbed"])
        if p == 0 and s == 0:
            continue
        rows.append((rel, p, s, t))

    ts = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    pct = (100.0 * total_ported / total_c) if total_c else 0.0

    data = {
        "generated": ts,
        "htop_source": str(HTOP_SRC),
        "c_functions_defined": total_c,
        "ported": total_ported,
        "stubbed": total_stubbed,
        "coverage_pct": round(pct, 2),
        "files_started": len(rows),
        "per_file": [
            {"cfile": rel, "ported": p, "stubbed": s, "defined": t}
            for (rel, p, s, t) in rows
        ],
    }

    OUT.parent.mkdir(parents=True, exist_ok=True)
    body_rows = "\n".join(
        f'          <tr><td><code>{html.escape(rel)}</code></td>'
        f"<td>{p}</td><td>{s}</td><td>{t}</td><td>{100.0 * p / t:.0f}%</td></tr>"
        for (rel, p, s, t) in rows
    )
    doc = f"""<!DOCTYPE html>
<!--PORT-REPORT-SCHEMA
Machine-readable dataset: <script id="port-report-data" type="application/json"> below.
  c_functions_defined = htop C function definitions (definitions only, not referenced libc symbols)
  ported              = ported fns (real body) whose name matches a defined C function
  stubbed             = scaffold fns (todo!()/unimplemented!() body) not yet ported
  coverage_pct        = 100 * ported / c_functions_defined (stubs do NOT count)
  per_file            = [{{cfile, ported, stubbed, defined}}] for each C file with >=1 ported or stubbed fn
-->
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <meta name="color-scheme" content="dark light">
  <meta name="description" content="htoprs port report — C-to-Rust coverage of the htop 3.5.1 port, per file and overall, derived from source at generation time.">
  <title>htoprs — Port Report</title>
  <link rel="preconnect" href="https://fonts.googleapis.com">
  <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
  <link href="https://fonts.googleapis.com/css2?family=Orbitron:wght@400;600;700;900&amp;family=Share+Tech+Mono&amp;display=swap" rel="stylesheet">
  <link rel="stylesheet" href="hud-static.css">
  <link rel="stylesheet" href="tutorial.css">
  <style>
    .tutorial-main {{ max-width: 68rem; }}
    .stat-grid {{ display: grid; grid-template-columns: repeat(auto-fill, minmax(11rem, 1fr)); gap: 0.5rem; margin: 1rem 0; }}
    .stat-card {{ border: 1px solid var(--border); border-left: 2px solid var(--cyan); padding: 0.65rem 0.85rem; background: color-mix(in srgb, var(--bg-card) 92%, transparent); border-radius: 2px; }}
    .stat-val {{ font-family: 'Orbitron', sans-serif; font-size: 20px; font-weight: 700; color: var(--accent); }}
    .stat-val.cyan {{ color: var(--cyan); }}
    .stat-val.green {{ color: var(--green); }}
    .stat-label {{ font-family: 'Share Tech Mono', monospace; font-size: 10px; text-transform: uppercase; letter-spacing: 1.2px; color: var(--text-dim); margin-top: 0.2rem; }}
    .arch-table {{ width: 100%; border-collapse: collapse; margin: 0.6rem 0; font-size: 12.5px; }}
    .arch-table th {{ background: var(--bg-secondary); color: var(--cyan); font-family: 'Orbitron', sans-serif; font-size: 10px; font-weight: 700; letter-spacing: 1px; text-transform: uppercase; text-align: left; padding: 6px 10px; border: 1px solid var(--border); }}
    .arch-table td {{ padding: 6px 10px; border: 1px solid var(--border); color: var(--text-dim); vertical-align: top; }}
    .arch-table td code {{ color: var(--accent-light); background: var(--bg); padding: 1px 4px; }}
  </style>
</head>
<body>
  <div class="app tutorial-app" id="portReportApp">
    <div class="crt-scanline" id="crtH" aria-hidden="true"></div>
    <div class="crt-scanline-v" id="crtV" aria-hidden="true"></div>

    <header class="tutorial-header">
      <div class="tutorial-header-inner">
        <div>
          <h1 class="tutorial-brand">// HTOPRS &mdash; PORT REPORT</h1>
          <nav class="tutorial-crumbs" aria-label="Breadcrumb">
            <span class="current">Port Report</span>
            <span class="sep">/</span>
            <a href="index.html">Docs</a>
            <span class="sep">/</span>
            <a href="report.html">Engineering report</a>
            <span class="sep">/</span>
            <a href="https://github.com/MenkeTechnologies/htoprs" target="_blank" rel="noopener noreferrer">GitHub</a>
          </nav>
          <p style="margin:0.35rem 0 0;font-family:'Share Tech Mono',monospace;font-size:11px;color:var(--text-dim);letter-spacing:0.03em;opacity:0.75;">
            Coverage of the htop 3.5.1 C spec &middot; generated {ts}
          </p>
        </div>
        <div class="tutorial-toolbar">
          <button type="button" class="btn btn-secondary" id="btnTheme" title="Toggle light/dark">Theme</button>
          <button type="button" class="btn btn-secondary active" id="btnCrt" title="CRT scanline overlay">CRT</button>
          <button type="button" class="btn btn-secondary active" id="btnNeon" title="Neon border pulse">Neon</button>
          <a class="btn btn-secondary" href="index.html">Docs</a>
          <a class="btn btn-secondary" href="report.html">Report</a>
        </div>
      </div>
    </header>

    <main class="tutorial-main">
      <h2 class="tutorial-title"><span class="step-hash">&gt;_</span>PORT COVERAGE</h2>
      <p class="tutorial-subtitle">C-to-Rust coverage of the htop <strong>3.5.1</strong> port, derived from the C source at <code>{html.escape(str(HTOP_SRC))}</code> and the Rust port under <code>src/ported/</code> at generation time. "Ported" = a <code>pub fn</code> whose name matches a function <em>defined</em> in the htop C source.</p>

      <div class="stat-grid">
        <div class="stat-card"><div class="stat-val green">{total_ported}</div><div class="stat-label">Fns ported</div></div>
        <div class="stat-card"><div class="stat-val">{total_stubbed}</div><div class="stat-label">Fns stubbed</div></div>
        <div class="stat-card"><div class="stat-val">{total_c}</div><div class="stat-label">C fns defined</div></div>
        <div class="stat-card"><div class="stat-val cyan">{pct:.2f}%</div><div class="stat-label">Coverage</div></div>
        <div class="stat-card"><div class="stat-val cyan">{len(rows)}</div><div class="stat-label">Files started</div></div>
      </div>

      <h2 class="tutorial-title"><span class="step-hash">~</span>PER-FILE</h2>
      <table class="arch-table">
        <thead><tr><th>C file</th><th>ported</th><th>stubbed</th><th>defined</th><th>coverage</th></tr></thead>
        <tbody>
{body_rows}
        </tbody>
      </table>
    </main>
  </div>
  <script id="port-report-data" type="application/json">
{json.dumps(data, indent=2)}
  </script>
  <script src="hud-theme.js"></script>
</body>
</html>
"""
    OUT.write_text(doc)
    print(f"Wrote {OUT.relative_to(ROOT)}")
    print(f"  {total_ported}/{total_c} C fns ported ({pct:.2f}%), "
          f"{total_stubbed} stubbed, {len(rows)} file(s) started")
    return 0


if __name__ == "__main__":
    sys.exit(main())
