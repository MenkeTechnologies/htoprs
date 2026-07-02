# PORT.md — Rules for Contributing to `htoprs`

`htoprs` is a **1:1 Rust port of htop**. The goal is 100% behavioral parity
with upstream htop. This is **not** a reimplementation, not a rewrite, not
"inspired by" htop. Every line of Rust code must trace back to a specific
line of upstream C code in `src/htop/Src/`.

---

## READ THIS FIRST — The Four Rules in One Screen

If you read nothing else in this file, read this. Every violation is
deleted on sight; the maintainer does not negotiate.

Do not use fully qualified names that are not in C.  C imports the names.  So Rust does too.  No imports inside functions, only imports at top of file organized by file.

### Rule 0 — ASK BEFORE INVENTING ANY NEW FN/STRUCT/STATIC NAME

**This rule overrides every other rule below.** If you (the bot) catch
yourself about to write a `fn`, `struct`, `enum`, `type`, or `static`
under `src/ported/` whose name does NOT exist in upstream htop C
source, you must **STOP and ASK THE MAINTAINER FIRST**. You do not
get to:

- "just add a tiny helper because it's only 3 lines"
- "factor out a Rust-only wrapper for borrow-checker reasons"
- "add a `_take`/`_set`/`_get`/`_clear`/`_is_some`/`_fill_*`/
  `_check_*` accessor for a thread_local"
- "split one C function into `foo` + `foo_impl` for argument routing"
- "add a Rust-only sentinel like `LEX_TABS_INITED` or `PARSER_*_DEPTH`"
- "introduce a `*State`/`*Table`/`*Builder`/`*Config`/`*Context`
  aggregate"
- "add an `error()`/`set_error()`/`check_limit()`/`check_recursion()`
  paranoia helper"

even if the helper looks "obviously useful," "trivially small,"
"locally scoped," "obviously safe," or "what any reasonable Rust
programmer would do." **None of those are reasons. Permission is
the only reason.**

**The required flow when you think a Rust-only helper is needed:**

1. **STOP**. Do not write the helper.
2. State to the maintainer: *"I'm about to add `fn <name>` (or
   `struct <Name>` / `static <NAME>`) under `src/ported/<file>.rs`
   because <one-sentence reason>. This name does not exist in
   upstream htop C. May I proceed?"*
3. **Wait for explicit permission.** Phrases that count as
   permission: "yes", "y", "ok", "go", "approved", "fine". Anything
   else — silence, "let me think", "why?", "what about X instead?"
   — is NOT permission.
4. If permission is granted, add the name AND immediately also add
   it to `tests/data/fake_fn_allowlist.txt` with the maintainer's
   approval recorded in the commit message ("approved 2026-MM-DD").
5. If permission is denied, the work goes back to either (a) using a
   real C-named port, (b) inlining the logic at call sites, or
   (c) abandoning the change.


**Test enforcement:** `tests/ported_fn_names_match_c.rs` rejects any
fn under `src/ported/` whose name is neither in
`docs/htop_c_functions.txt` nor in
`tests/data/fake_fn_allowlist.txt`. Adding a new name to the
allowlist without prior maintainer approval is itself a violation —
the allowlist is not a free pass, it's the audit trail of granted
exceptions.

---

**Rule A — Names must exist in upstream htop C.** This applies to
**every declaration** in `src/ported/`, not just functions:

| Rust decl                                  | Must exist in C as                                          | Verify with                                                                                |
