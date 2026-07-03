//! #6 — threshold alerts.
//!
//! htop has none. A [`Rule`] fires when a PID holds a metric at/above a
//! threshold for `for_ticks` consecutive refreshes (debounced, so a single
//! spike does not fire). The firing PID is recolored at the
//! `Ncurses::to_color` chokepoint in the real tree.

use std::collections::HashMap;

use crate::extensions::model::Proc;

/// Metric a rule watches.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Metric {
    Cpu,
    MemKb,
}

/// A threshold rule.
#[derive(Clone, Debug, PartialEq)]
pub struct Rule {
    pub name: String,
    pub metric: Metric,
    /// Fire when value >= this.
    pub threshold: f64,
    /// Consecutive over-threshold refreshes required before firing.
    pub for_ticks: u32,
}

/// A rule currently firing for a PID.
#[derive(Clone, Debug, PartialEq)]
pub struct Firing {
    pub rule: String,
    pub pid: u32,
    pub value: f64,
    /// How many consecutive ticks it has been over.
    pub sustained: u32,
}

/// Stateful evaluator: tracks the consecutive-over counter per (rule, PID).
pub struct AlertEngine {
    rules: Vec<Rule>,
    /// (rule_index, pid) -> consecutive over-threshold count.
    over: HashMap<(usize, u32), u32>,
}

impl AlertEngine {
    pub fn new(rules: Vec<Rule>) -> Self {
        AlertEngine {
            rules,
            over: HashMap::new(),
        }
    }

    fn value(metric: Metric, p: &Proc) -> f64 {
        match metric {
            Metric::Cpu => p.cpu as f64,
            Metric::MemKb => p.mem_kb as f64,
        }
    }

    /// Feed one refresh; return every rule/PID pair that is now firing.
    pub fn evaluate(&mut self, table: &[Proc]) -> Vec<Firing> {
        let live: Vec<u32> = table.iter().map(|p| p.pid).collect();
        let mut firing = Vec::new();

        for (ri, rule) in self.rules.iter().enumerate() {
            for p in table {
                let v = Self::value(rule.metric, p);
                let key = (ri, p.pid);
                if v >= rule.threshold {
                    let c = self.over.entry(key).or_insert(0);
                    *c += 1;
                    if *c >= rule.for_ticks {
                        firing.push(Firing {
                            rule: rule.name.clone(),
                            pid: p.pid,
                            value: v,
                            sustained: *c,
                        });
                    }
                } else {
                    self.over.remove(&key);
                }
            }
        }
        // drop counters for PIDs that left the table
        self.over.retain(|(_, pid), _| live.contains(pid));
        firing
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extensions::model::Proc;

    fn proc(pid: u32, cpu: f32) -> Proc {
        Proc {
            pid,
            ppid: 1,
            user: "u".into(),
            comm: "c".into(),
            cmdline: "c".into(),
            state: 'R',
            cpu,
            mem_kb: 1,
        }
    }

    #[test]
    fn debounces_single_spike() {
        let mut e = AlertEngine::new(vec![Rule {
            name: "hot".into(),
            metric: Metric::Cpu,
            threshold: 80.0,
            for_ticks: 3,
        }]);
        assert!(e.evaluate(&[proc(1, 90.0)]).is_empty()); // tick 1
        assert!(e.evaluate(&[proc(1, 90.0)]).is_empty()); // tick 2
        let f = e.evaluate(&[proc(1, 90.0)]); // tick 3 -> fire
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].pid, 1);
        assert_eq!(f[0].sustained, 3);
    }

    #[test]
    fn dropping_below_resets_counter() {
        let mut e = AlertEngine::new(vec![Rule {
            name: "hot".into(),
            metric: Metric::Cpu,
            threshold: 80.0,
            for_ticks: 2,
        }]);
        e.evaluate(&[proc(1, 90.0)]); // count 1
        e.evaluate(&[proc(1, 10.0)]); // reset
        assert!(e.evaluate(&[proc(1, 90.0)]).is_empty()); // count 1 again, not 2
    }
}
