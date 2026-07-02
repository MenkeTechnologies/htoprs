#!/bin/zsh
# Regenerate tests/data/htop_c_fn_names.txt from the upstream htop
# C source. Run after pulling new upstream commits.
#
# Usage:
#   HTOP_C_SOURCE=~/forkedRepos/htop ./tests/data/extract_c_fn_names.sh
# or just:
#   ./tests/data/extract_c_fn_names.sh
#   (defaults to ~/forkedRepos/htop)
#
# Output format: one entry per line, `<basename>:<fn_name>`. The
# basename is the C file (e.g. `XUtils.c`) so the drift-detection
# test can verify Rust ports landed in the matching file (rename
# detection).
#
# The list is checked into git so the drift-detection test and the
# build.rs port-purity gate don't depend on a local checkout of
# htop's source.

set -e
cd "$(dirname "$0")/../.."

HTOP_SRC="${HTOP_C_SOURCE:-$HOME/forkedRepos/htop}"
if [[ ! -d "$HTOP_SRC" ]]; then
    print -u2 "ERROR: htop source not found at $HTOP_SRC"
    print -u2 "Set HTOP_C_SOURCE to override."
    exit 1
fi

OUT=tests/data/htop_c_fn_names.txt

{
    print '# Function names extracted from htop upstream C source.'
    print '# Format: <basename>:<fn_name>'
    print '# Regenerate via tests/data/extract_c_fn_names.sh.'
    print "# Source: $HTOP_SRC ($(find "$HTOP_SRC" -name "*.c" | wc -l | tr -d ' ') files)"
    print "# Generated: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
    print ''

    # For each .c file, extract identifiers preceding `(` and emit
    # `basename:fn`. Strip C keywords. Platform dirs (linux/, darwin/,
    # etc.) sometimes share a basename-scoped fn across platforms; we
    # keep all occurrences so the gate can check "did the port land in
    # any C file containing this fn".
    find "$HTOP_SRC" -name "*.c" -type f | while read -r f; do
        base="${f:t}"
        grep -oE '[a-zA-Z_][a-zA-Z_0-9]*\(' "$f" 2>/dev/null \
            | sed 's/($//' \
            | grep -vE '^(if|while|for|switch|return|sizeof|typedef|do|else|case|default|goto|break|continue)$' \
            | sort -u \
            | sed "s|^|${base}:|"
    done | sort -u
} > "$OUT"

LINES=$(grep -cv '^#' "$OUT" | head -1)
print "Wrote $OUT ($LINES (file,fn) entries)"
