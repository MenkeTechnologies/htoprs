//! htoprs — a faithful Rust port of htop.
//!
//! The C source at `~/forkedRepos/htop` (v3.5.1) is the spec. Every
//! function under [`ported`] ports a specific htop C function, cited
//! by `<File>.c:<line>` in its doc comment. The port-purity gate in
//! `build.rs` rejects any free `fn` under `src/ported/` whose name
//! has no counterpart in the htop C source (snapshotted at
//! `tests/data/htop_c_fn_names.txt`).
//!
//! # Clippy and faithful ports
//!
//! CI runs `cargo clippy --all-targets -- -D warnings`. A handful of
//! idiom lints are allowed crate-wide because the flagged code is a
//! literal translation of the htop C source, and rewriting it into the
//! idiomatic Rust clippy prefers would either diverge from the spec or
//! change behavior:
//!
//! * `needless_range_loop` / `explicit_counter_loop` — dual-index
//!   `for (i, j) …` loops (`strncpy`, `RichString` fills) mirror the C
//!   pointer walks; iterator rewrites lose the 1:1 line mapping.
//! * `manual_div_ceil` — `(cpus + 1) / 2` is the C half-split formula.
//! * `implicit_saturating_sub` — `count > MAX ? count - MAX : 0` is the
//!   C ternary; `saturating_sub` reads differently from the source.
//! * `manual_pattern_char_comparison` — explicit `\n`/`\r` compares
//!   match the C trailing-newline strip.
//! * `neg_cmp_op_on_partial_ord` — `!(rate >= 0.0)` is a deliberate
//!   NaN guard from `Row.c`; `rate < 0.0` would treat NaN as valid.
//! * `identity_op` / `int_plus_one` — test assertions spell out the C
//!   arithmetic (`0 + 0 - scrollV + 1`, `len >= n + 1`) for clarity.
//! * `doc_lazy_continuation` — ported doc comments carry C-snippet
//!   bullet lists whose wrapped lines trip the lint.
//! * `field_reassign_with_default` — htop's `*_init` routines set fields
//!   one at a time on a freshly `calloc`'d struct; the port mirrors that
//!   line-for-line rather than collapsing into a struct literal.
//! * `unnecessary_unwrap` — `if x.is_some() { x.unwrap() }` mirrors the
//!   C `if (ptr) { ptr->… }` null-check-then-deref shape.
//! * `manual_c_str_literals` — `b"kern.osrelease\0"` are the literal C
//!   string constants passed to `sysctlbyname`; `c"…"` hides the NUL the
//!   source spells out.
//! * `not_unsafe_ptr_arg_deref` — ported helpers keep htop's raw-pointer
//!   parameter signatures; marking them `unsafe` would change the API.
//! * `too_many_arguments` — ported functions keep htop's C parameter
//!   lists verbatim.
//! * `erasing_op` — test assertions spell out C arithmetic that folds to
//!   zero (`(-20 + 20) / 5` = the nice→ioprio mapping) for clarity.
//! * `needless_late_init` — a `buffer` declared then filled inside a
//!   `match`/`switch` mirrors htop's `char* buffer; switch(field){…}`.
#![allow(clippy::needless_range_loop)]
#![allow(clippy::explicit_counter_loop)]
#![allow(clippy::manual_div_ceil)]
#![allow(clippy::implicit_saturating_sub)]
#![allow(clippy::manual_pattern_char_comparison)]
#![allow(clippy::neg_cmp_op_on_partial_ord)]
#![allow(clippy::identity_op)]
#![allow(clippy::int_plus_one)]
#![allow(clippy::doc_lazy_continuation)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::unnecessary_unwrap)]
#![allow(clippy::manual_c_str_literals)]
#![allow(clippy::not_unsafe_ptr_arg_deref)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::erasing_op)]
#![allow(clippy::needless_late_init)]

pub mod extensions;
pub mod ported;
