//! #3 — capture + diff a process table.
//!
//! htop keeps no record of any moment. This dumps the table to JSON (reusing
//! the `extensions::prefs` serde-json pattern) and diffs two captures by PID
//! for incident forensics: what appeared, what died, what changed.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::extensions::model::Proc;

/// A frozen process table plus the tick it was taken at.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub tick: u64,
    pub procs: Vec<Proc>,
}

impl Snapshot {
    pub fn capture(tick: u64, table: &[Proc]) -> Self {
        Snapshot {
            tick,
            procs: table.to_vec(),
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("Proc serializes")
    }

    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }
}

/// A PID present in both snapshots whose observable fields moved.
#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    pub pid: u32,
    pub before: Proc,
    pub after: Proc,
}

/// Result of diffing `a` (before) against `b` (after).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Diff {
    pub added: Vec<Proc>,
    pub removed: Vec<Proc>,
    pub changed: Vec<Change>,
}

/// Diff two snapshots, keyed by PID. `changed` reports rows whose cpu, mem,
/// state, or cmdline differ (a PID reused for a new command shows up here).
pub fn diff(a: &Snapshot, b: &Snapshot) -> Diff {
    let before: BTreeMap<u32, &Proc> = a.procs.iter().map(|p| (p.pid, p)).collect();
    let after: BTreeMap<u32, &Proc> = b.procs.iter().map(|p| (p.pid, p)).collect();

    let mut d = Diff::default();
    for (pid, bp) in &after {
        match before.get(pid) {
            None => d.added.push((*bp).clone()),
            Some(ap) => {
                if ap.cpu != bp.cpu
                    || ap.mem_kb != bp.mem_kb
                    || ap.state != bp.state
                    || ap.cmdline != bp.cmdline
                {
                    d.changed.push(Change {
                        pid: *pid,
                        before: (*ap).clone(),
                        after: (*bp).clone(),
                    });
                }
            }
        }
    }
    for (pid, ap) in &before {
        if !after.contains_key(pid) {
            d.removed.push((*ap).clone());
        }
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extensions::model::synthetic_table;

    #[test]
    fn json_roundtrips() {
        let s = Snapshot::capture(2, &synthetic_table(2));
        let back = Snapshot::from_json(&s.to_json()).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn detects_added_and_removed() {
        let a = Snapshot::capture(0, &synthetic_table(0)); // has pid 500
        let b = Snapshot::capture(4, &synthetic_table(4)); // no pid 500
        let d = diff(&a, &b);
        assert!(d.removed.iter().any(|p| p.pid == 500));
        assert!(!d.added.iter().any(|p| p.pid == 500));
    }

    #[test]
    fn identical_snapshots_have_empty_diff() {
        let a = Snapshot::capture(1, &synthetic_table(1));
        let d = diff(&a, &a);
        assert!(d.added.is_empty() && d.removed.is_empty() && d.changed.is_empty());
    }
}
