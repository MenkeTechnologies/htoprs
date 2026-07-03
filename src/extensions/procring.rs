//! #1 — per-process time-series ring + sparkline column.
//!
//! The single capability htop structurally lacks: a scrollable in-TUI history
//! of what each PID was doing. `src/ported/history.rs` is only the *command
//! line* ring (`history.rs:1`), so this is genuinely new state.
//!
//! [`ProcRing`] keeps a bounded CPU/mem ring per live PID, advanced once per
//! refresh via [`ProcRing::record`], and dropping rings for PIDs that left the
//! table so memory is bounded by live process count, not history depth.

use std::collections::HashMap;

use crate::extensions::braille;
use crate::extensions::model::Proc;

/// One recorded sample.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub cpu: f32,
    pub mem_kb: u64,
}

/// Fixed-capacity oldest-first ring of samples.
#[derive(Clone, Debug)]
struct Ring {
    cap: usize,
    buf: Vec<Sample>,
}

impl Ring {
    fn new(cap: usize) -> Self {
        Ring {
            cap,
            buf: Vec::with_capacity(cap),
        }
    }

    fn push(&mut self, s: Sample) {
        if self.buf.len() == self.cap {
            self.buf.remove(0);
        }
        self.buf.push(s);
    }
}

/// Per-PID history store.
pub struct ProcRing {
    cap: usize,
    map: HashMap<u32, Ring>,
}

impl ProcRing {
    /// New store keeping the last `cap` samples per PID.
    pub fn new(cap: usize) -> Self {
        ProcRing {
            cap: cap.max(1),
            map: HashMap::new(),
        }
    }

    /// Advance every live PID by one sample and evict rings for dead PIDs.
    pub fn record(&mut self, table: &[Proc]) {
        let mut seen = Vec::with_capacity(table.len());
        for p in table {
            seen.push(p.pid);
            self.map
                .entry(p.pid)
                .or_insert_with(|| Ring::new(self.cap))
                .push(Sample {
                    cpu: p.cpu,
                    mem_kb: p.mem_kb,
                });
        }
        self.map.retain(|pid, _| seen.contains(pid));
    }

    /// Number of PIDs currently held.
    pub fn tracked(&self) -> usize {
        self.map.len()
    }

    /// CPU history for `pid`, oldest-first (empty if untracked).
    pub fn cpu_series(&self, pid: u32) -> Vec<f32> {
        self.map
            .get(&pid)
            .map(|r| r.buf.iter().map(|s| s.cpu).collect())
            .unwrap_or_default()
    }

    /// Resident-memory history for `pid`, oldest-first.
    pub fn mem_series(&self, pid: u32) -> Vec<u64> {
        self.map
            .get(&pid)
            .map(|r| r.buf.iter().map(|s| s.mem_kb).collect())
            .unwrap_or_default()
    }

    /// Right-aligned CPU sparkline for `pid`, exactly `width` glyphs, scaled
    /// to `max` percent. Newest sample sits at the right edge.
    pub fn cpu_sparkline(&self, pid: u32, width: usize, max: f32) -> String {
        let series = self.cpu_series(pid);
        let tail = if series.len() > width {
            &series[series.len() - width..]
        } else {
            &series[..]
        };
        let pad = width.saturating_sub(tail.len());
        let mut out = " ".repeat(pad);
        out.push_str(&braille::spark(tail, max));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extensions::model::synthetic_table;

    #[test]
    fn records_and_bounds_by_capacity() {
        let mut r = ProcRing::new(3);
        for t in 0..10 {
            r.record(&synthetic_table(t));
        }
        // init (pid 1) is always present; ring capped at 3
        assert_eq!(r.cpu_series(1).len(), 3);
    }

    #[test]
    fn evicts_dead_pids() {
        let mut r = ProcRing::new(8);
        r.record(&synthetic_table(0)); // pid 500 present
        assert!(r.cpu_series(500).len() == 1);
        r.record(&synthetic_table(4)); // pid 500 gone
        assert!(r.cpu_series(500).is_empty());
    }

    #[test]
    fn sparkline_is_exactly_width_and_right_aligned() {
        let mut r = ProcRing::new(64);
        for t in 0..3 {
            r.record(&synthetic_table(t));
        }
        let s = r.cpu_sparkline(1, 10, 100.0);
        assert_eq!(s.chars().count(), 10);
        // only 3 samples so far -> left padded with spaces
        assert!(s.starts_with("       "));
    }
}
