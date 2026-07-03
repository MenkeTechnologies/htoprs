//! Minimal process model standing in for htoprs's real `Process`.
//!
//! In the real tree these fields are read off the ported `Process` struct
//! (`src/ported/process.rs`); here they are the subset the extensions need.

use serde::{Deserialize, Serialize};

/// One row of the process table.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Proc {
    pub pid: u32,
    pub ppid: u32,
    pub user: String,
    /// Short name (`comm`).
    pub comm: String,
    /// Full command line.
    pub cmdline: String,
    /// Process state char (`R`, `S`, `D`, `Z`, ...).
    pub state: char,
    /// CPU usage percent (0..=100 per core-normalized, may exceed 100 on SMP).
    pub cpu: f32,
    /// Resident memory in KiB.
    pub mem_kb: u64,
}

/// Deterministic synthetic process table for tick `t`.
///
/// Real htoprs samples the OS; the prototypes take a table as input, so this
/// generator drives the demo and tests reproducibly (no clock, no RNG). Load
/// varies smoothly with `t`; pid 500 appears only on some ticks so snapshot
/// diff (#3) and ring eviction (#1) have something to observe.
pub fn synthetic_table(t: u64) -> Vec<Proc> {
    const BASE: &[(u32, u32, &str, &str, &str, char)] = &[
        (1, 0, "root", "init", "/sbin/init", 'S'),
        (100, 1, "root", "kthreadd", "[kthreadd]", 'S'),
        (200, 1, "user", "zsh", "-zsh", 'S'),
        (201, 200, "user", "cargo", "cargo build --workspace", 'R'),
        (
            202,
            201,
            "user",
            "rustc",
            "rustc --edition 2021 src/lib.rs",
            'R',
        ),
        (300, 1, "user", "firefox", "/usr/lib/firefox/firefox", 'S'),
        (
            301,
            300,
            "user",
            "firefox-tab",
            "firefox -contentproc -childID 7",
            'R',
        ),
        (
            400,
            1,
            "postgres",
            "postgres",
            "postgres -D /var/lib/pg/data",
            'S',
        ),
    ];

    let mut out: Vec<Proc> = BASE
        .iter()
        .enumerate()
        .map(|(i, &(pid, ppid, user, comm, cmd, state))| {
            let phase = t as f32 * 0.5 + i as f32;
            let swing = (phase.sin() * 0.5 + 0.5) * 90.0;
            let weight = ((i as f32 % 3.0) + 1.0) / 3.0;
            let cpu = (swing * weight).abs();
            let mem_kb = 10_000 + (i as u64 * 40_000) + (t % 8) * 2_000;
            Proc {
                pid,
                ppid,
                user: user.into(),
                comm: comm.into(),
                cmdline: cmd.into(),
                state,
                cpu,
                mem_kb,
            }
        })
        .collect();

    if t % 6 < 3 {
        out.push(Proc {
            pid: 500,
            ppid: 200,
            user: "user".into(),
            comm: "ephemeral".into(),
            cmdline: "sh -c 'sleep 1'".into(),
            state: 'R',
            cpu: 5.0 + (t % 3) as f32 * 10.0,
            mem_kb: 4_096,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthetic_is_deterministic() {
        assert_eq!(synthetic_table(3), synthetic_table(3));
    }

    #[test]
    fn ephemeral_pid_toggles() {
        assert!(synthetic_table(0).iter().any(|p| p.pid == 500));
        assert!(!synthetic_table(4).iter().any(|p| p.pid == 500));
    }
}
