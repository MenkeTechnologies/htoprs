//! #5 — export the current view to JSON / CSV.
//!
//! htop is TUI-only; nothing leaves the screen machine-readable. One keypress
//! dumps the (already filtered/sorted) table to a pipe-friendly format.

use crate::extensions::model::Proc;

/// Serialize `procs` as a pretty JSON array.
pub fn to_json(procs: &[Proc]) -> String {
    serde_json::to_string_pretty(procs).expect("Proc serializes")
}

/// RFC-4180-style CSV with a header row.
pub fn to_csv(procs: &[Proc]) -> String {
    let mut out = String::from("pid,ppid,user,comm,cmdline,state,cpu,mem_kb\n");
    for p in procs {
        out.push_str(&format!(
            "{},{},{},{},{},{},{:.1},{}\n",
            p.pid,
            p.ppid,
            csv_field(&p.user),
            csv_field(&p.comm),
            csv_field(&p.cmdline),
            p.state,
            p.cpu,
            p.mem_kb,
        ));
    }
    out
}

/// Quote a field iff it contains a comma, quote, CR, or LF; double inner quotes.
fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extensions::model::{synthetic_table, Proc};

    #[test]
    fn json_is_valid_array() {
        let j = to_json(&synthetic_table(0));
        let v: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert!(v.is_array());
    }

    #[test]
    fn csv_has_header_and_row_per_proc() {
        let table = synthetic_table(4); // no pid 500 -> 8 rows
        let csv = to_csv(&table);
        assert_eq!(csv.lines().count(), table.len() + 1);
        assert!(csv.starts_with("pid,ppid,user,comm,cmdline,state,cpu,mem_kb\n"));
    }

    #[test]
    fn csv_quotes_and_escapes_commas_and_quotes() {
        let p = Proc {
            pid: 9,
            ppid: 1,
            user: "u".into(),
            comm: "x".into(),
            cmdline: "sh -c \"a,b\"".into(),
            state: 'R',
            cpu: 1.0,
            mem_kb: 1,
        };
        let csv = to_csv(&[p]);
        assert!(csv.contains("\"sh -c \"\"a,b\"\"\""));
    }
}
