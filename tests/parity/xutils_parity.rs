//! XUtils parity: htoprs's ported pure functions vs htop's REAL `XUtils.c`.
//!
//! A tiny C reference harness (`cref/htop_cref.c`) is compiled against htop's
//! genuine `XUtils.c` (version 3.5.x) and invoked per input; the Rust port is
//! called with the same input and the two outputs are compared byte-for-byte.
//! This is the zshrs/ztmux parity model applied to library functions instead of
//! a whole binary.
//!
//! Skips (stays green) when the htop C source (`HTOP_C_SOURCE`, default
//! `~/forkedRepos/htop`) or a C compiler is unavailable — so CI without them
//! passes, while a dev box with the source runs the real diff.

use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

use htoprs::ported::xutils;

/// htop C source tree (env override, then the canonical checkout).
fn htop_c_source() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("HTOP_C_SOURCE") {
        let p = PathBuf::from(p);
        return p.join("XUtils.c").exists().then_some(p);
    }
    let p = PathBuf::from(std::env::var("HOME").ok()?).join("forkedRepos/htop");
    p.join("XUtils.c").exists().then_some(p)
}

/// Compile the C reference harness once; cache the binary path. `None` if the
/// htop source or a C compiler is missing / the build fails.
fn cref_bin() -> Option<&'static PathBuf> {
    static BIN: OnceLock<Option<PathBuf>> = OnceLock::new();
    BIN.get_or_init(|| {
        let src = htop_c_source()?;
        let cref_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/parity/cref");
        let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("htop_cref");
        let cc = std::env::var("CC").unwrap_or_else(|_| "cc".to_string());
        let status = Command::new(&cc)
            .args(["-std=c11", "-O1", "-DHEADER_CRT"])
            .arg("-I")
            .arg(&cref_dir)
            .arg("-I")
            .arg(&src)
            .arg("-o")
            .arg(&out)
            .arg(cref_dir.join("htop_cref.c"))
            .arg(src.join("XUtils.c"))
            .status()
            .ok()?;
        (status.success() && out.exists()).then_some(out)
    })
    .as_ref()
}

/// Run the C reference harness; `None` if it's unavailable (→ skip the case).
fn cref(args: &[&str]) -> Option<String> {
    let bin = cref_bin()?;
    let o = Command::new(bin).args(args).output().ok()?;
    assert!(
        o.status.success(),
        "cref {:?} failed: {}",
        args,
        String::from_utf8_lossy(&o.stderr)
    );
    Some(String::from_utf8_lossy(&o.stdout).into_owned())
}

/// Assert the Rust port's rendered output equals the C reference for `args`.
fn eq(args: &[&str], rust: String) {
    let Some(c) = cref(args) else { return };
    assert_eq!(
        c.trim_end_matches('\n'),
        rust.trim_end_matches('\n'),
        "XUtils parity divergence for {:?}\n--- htop C ---\n{}\n--- htoprs ---\n{}",
        args,
        c,
        rust,
    );
}

fn split_render(v: &[String]) -> String {
    let mut s = format!("n={}\n", v.len());
    for t in v {
        s += &format!("[{t}]\n");
    }
    s
}

#[test]
fn count_digits() {
    for &(n, base) in &[
        (0usize, 10usize),
        (1, 10),
        (9, 10),
        (10, 10),
        (99, 10),
        (100, 10),
        (1234567890, 10),
        (usize::MAX, 10),
        (0, 2),
        (1, 2),
        (2, 2),
        (255, 2),
        (256, 2),
        (0, 16),
        (15, 16),
        (16, 16),
        (255, 16),
        (256, 16),
        (0, 8),
        (7, 8),
        (8, 8),
        (usize::MAX, 16),
    ] {
        eq(
            &["countDigits", &n.to_string(), &base.to_string()],
            format!("{}", xutils::countDigits(n, base)),
        );
    }
}

#[test]
fn count_trailing_zeros() {
    let mut cases = vec![
        0u32,
        1,
        2,
        3,
        4,
        6,
        8,
        12,
        16,
        255,
        256,
        1024,
        0x8000_0000,
        u32::MAX,
    ];
    for i in 0..32 {
        cases.push(1u32 << i);
    }
    for x in cases {
        eq(
            &["countTrailingZeros", &x.to_string()],
            format!("{}", xutils::countTrailingZeros(x)),
        );
    }
}

#[test]
fn compare_real_numbers() {
    for &(a, b) in &[
        (1.0, 2.0),
        (2.0, 1.0),
        (1.0, 1.0),
        (0.0, 0.0),
        (-1.0, 1.0),
        (1.5, 1.5),
        (-0.0, 0.0),
        (1e300, 1e-300),
        (0.1, 0.2),
    ] {
        eq(
            &["compareRealNumbers", &format!("{a}"), &format!("{b}")],
            format!("{}", xutils::compareRealNumbers(a, b)),
        );
    }
}

#[test]
fn sum_positive_values() {
    let cases: &[&[f64]] = &[
        &[],
        &[1.0, 2.0, 3.0],
        &[-1.0, -2.0, -3.0],
        &[1.0, -2.0, 3.0, -4.0, 5.0],
        &[0.0, 0.0, 0.0],
        &[1.5, 2.25, -0.5, 100.125],
    ];
    for arr in cases {
        let joined = arr
            .iter()
            .map(|x| format!("{x}"))
            .collect::<Vec<_>>()
            .join(",");
        let Some(c) = cref(&["sumPositiveValues", &joined]) else {
            return;
        };
        let c_val: f64 = c.trim().parse().expect("cref f64");
        let r_val = xutils::sumPositiveValues(arr);
        assert_eq!(
            c_val, r_val,
            "sumPositiveValues divergence for {arr:?}: C={c_val} rust={r_val}"
        );
    }
}

#[test]
fn string_cat() {
    for &(a, b) in &[
        ("foo", "bar"),
        ("", "x"),
        ("x", ""),
        ("", ""),
        ("a b", "c d"),
        ("héllo", "wörld"),
    ] {
        eq(
            &["String_cat", a, b],
            format!("[{}]", xutils::String_cat(a, b)),
        );
    }
}

#[test]
fn string_trim() {
    for s in &[
        "  hi  ",
        "",
        "   ",
        "\t hi \n",
        "no-trim",
        " leading",
        "trailing ",
        "  a b c  ",
    ] {
        eq(&["String_trim", s], format!("[{}]", xutils::String_trim(s)));
    }
}

#[test]
fn string_contains_i() {
    for &(a, b, multi) in &[
        ("Hello World", "world", false),
        ("Hello World", "WORLD", false),
        ("abc", "d", false),
        ("abc", "", false),
        ("", "x", false),
        ("Hello World", "o w", true),
        ("aAaA", "aa", false),
    ] {
        eq(
            &["String_contains_i", a, b, if multi { "1" } else { "0" }],
            format!(
                "{}",
                if xutils::String_contains_i(a, b, multi) {
                    1
                } else {
                    0
                }
            ),
        );
    }
}

#[test]
fn string_split() {
    for &(s, sep) in &[
        ("a,b,c", ','),
        ("a", ','),
        ("", ','),
        (",", ','),
        ("a,", ','),
        (",a", ','),
        ("a,,b", ','),
        ("one two three", ' '),
        ("x:y:z:", ':'),
    ] {
        eq(
            &["String_split", s, &sep.to_string()],
            split_render(&xutils::String_split(s, sep)),
        );
    }
}

#[test]
fn string_split_first() {
    for &(s, sep) in &[
        ("a,b,c", ','),
        ("a", ','),
        ("", ','),
        ("a,", ','),
        (",a", ','),
        ("key=value=extra", '='),
        ("nosep", '/'),
    ] {
        eq(
            &["String_splitFirst", s, &sep.to_string()],
            split_render(&xutils::String_splitFirst(s, sep)),
        );
    }
}
