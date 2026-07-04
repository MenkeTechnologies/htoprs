//! #8 — live aggregation / pivot rollups.
//!
//! htop has a tree view but no way to *aggregate* the process table: total CPU
//! and memory grouped by user, by command name, or by parent. This rolls the
//! live [`Proc`] rows up on a chosen key and sorts the groups by CPU, so "which
//! user is burning the box" or "how much RAM is all my `chrome` costing" is one
//! keystroke, not mental arithmetic across dozens of rows.
//!
//! Pure logic: [`aggregate`] takes the bridged table and a [`GroupBy`] and
//! returns sorted [`Group`]s. The running-TUI wiring (modal, hotkey, render) is
//! in [`super::panels`], the same split every other extension uses.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::extensions::model::Proc;

/// Which key the process table is rolled up on. Cycled by `Tab` in the modal
/// and persisted to prefs.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum GroupBy {
    /// Group by owning user name.
    #[default]
    User,
    /// Group by command name (`comm`).
    Command,
    /// Group by parent PID (labeled with the parent's command when known).
    Parent,
}

impl GroupBy {
    /// Short label for the modal header.
    pub fn label(self) -> &'static str {
        match self {
            GroupBy::User => "user",
            GroupBy::Command => "command",
            GroupBy::Parent => "parent",
        }
    }

    /// Next key in the cycle User → Command → Parent → User.
    pub fn next(self) -> GroupBy {
        match self {
            GroupBy::User => GroupBy::Command,
            GroupBy::Command => GroupBy::Parent,
            GroupBy::Parent => GroupBy::User,
        }
    }
}

/// One rolled-up group: its key plus the summed process count, CPU, and memory.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    /// The group key rendered for display (user name, command, or parent).
    pub key: String,
    /// How many processes fell into this group.
    pub count: usize,
    /// Summed CPU percentage across the group.
    pub cpu: f32,
    /// Summed resident memory in KB across the group.
    pub mem_kb: u64,
}

/// Roll `table` up on `by`, returning groups sorted by CPU descending (ties
/// broken by memory descending, then key ascending for a stable order). For
/// [`GroupBy::Parent`], a parent PID present in the table is labeled
/// `"pid comm"`; an absent parent (e.g. reaped/`0`) is labeled by its number.
pub fn aggregate(table: &[Proc], by: GroupBy) -> Vec<Group> {
    // Parent labels need a pid → comm lookup; build it once when grouping by
    // parent so the O(n) rollup stays O(n).
    let comm_by_pid: HashMap<u32, &str> = if by == GroupBy::Parent {
        table.iter().map(|p| (p.pid, p.comm.as_str())).collect()
    } else {
        HashMap::new()
    };

    let mut groups: HashMap<String, Group> = HashMap::new();
    for p in table {
        let key = match by {
            GroupBy::User => p.user.clone(),
            GroupBy::Command => p.comm.clone(),
            GroupBy::Parent => match comm_by_pid.get(&p.ppid) {
                Some(comm) => format!("{} {}", p.ppid, comm),
                None => p.ppid.to_string(),
            },
        };
        let g = groups.entry(key.clone()).or_insert(Group {
            key,
            count: 0,
            cpu: 0.0,
            mem_kb: 0,
        });
        g.count += 1;
        g.cpu += p.cpu;
        g.mem_kb += p.mem_kb;
    }

    let mut out: Vec<Group> = groups.into_values().collect();
    out.sort_by(|a, b| {
        b.cpu
            .total_cmp(&a.cpu)
            .then(b.mem_kb.cmp(&a.mem_kb))
            .then(a.key.cmp(&b.key))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc(pid: u32, ppid: u32, user: &str, comm: &str, cpu: f32, mem_kb: u64) -> Proc {
        Proc {
            pid,
            ppid,
            user: user.into(),
            comm: comm.into(),
            cmdline: comm.into(),
            state: 'R',
            cpu,
            mem_kb,
        }
    }

    fn table() -> Vec<Proc> {
        vec![
            proc(1, 0, "root", "init", 0.0, 100),
            proc(2, 1, "root", "bash", 5.0, 200),
            proc(3, 1, "jacob", "chrome", 30.0, 4000),
            proc(4, 1, "jacob", "chrome", 20.0, 3000),
            proc(5, 2, "jacob", "vim", 1.0, 500),
        ]
    }

    #[test]
    fn groups_by_user_sum_and_sort() {
        let g = aggregate(&table(), GroupBy::User);
        // jacob: 30+20+1 = 51 cpu; root: 0+5 = 5 cpu. jacob sorts first.
        assert_eq!(g[0].key, "jacob");
        assert_eq!(g[0].count, 3);
        assert_eq!(g[0].cpu, 51.0);
        assert_eq!(g[0].mem_kb, 7500);
        assert_eq!(g[1].key, "root");
        assert_eq!(g[1].cpu, 5.0);
    }

    #[test]
    fn groups_by_command_merges_same_comm() {
        let g = aggregate(&table(), GroupBy::Command);
        // two chrome procs merge into one 50-cpu group at the top.
        assert_eq!(g[0].key, "chrome");
        assert_eq!(g[0].count, 2);
        assert_eq!(g[0].cpu, 50.0);
    }

    #[test]
    fn groups_by_parent_labels_known_parent() {
        let g = aggregate(&table(), GroupBy::Parent);
        // ppid 1 (init) owns bash+chrome+chrome = 55 cpu, top group.
        assert_eq!(g[0].key, "1 init");
        assert_eq!(g[0].count, 3);
        assert_eq!(g[0].cpu, 55.0);
        // pid 1's parent is 0, which is absent from the table → numeric label.
        assert!(g.iter().any(|x| x.key == "0"));
    }

    #[test]
    fn empty_table_is_empty() {
        assert!(aggregate(&[], GroupBy::User).is_empty());
    }
}
