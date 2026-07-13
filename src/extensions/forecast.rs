//! #9 — per-process resource-exhaustion horizon.
//!
//! htop shows what memory a process holds *now*; nothing in it answers "and
//! when does that become a problem". This module is the leading indicator: each
//! refresh it fits a least-squares trend on a PID's bounded memory ring
//! ([`super::procring`]) and projects the wall-clock time until that PID's
//! resident set crosses its own real ceiling.
//!
//! The ceiling is the actual enforced limit, in precedence order
//! ([`select_ceiling`]): a Linux cgroup-v2 `memory.max`, else the process
//! address-space `RLIMIT_AS`, else total machine RAM. [`Ceiling::detect`] reads
//! it once and caches it (the enforced limit does not move during a session).
//!
//! [`Trend`] is deliberately pure — a hand-rolled ordinary-least-squares slope
//! over the sample indices, no clock, no I/O — so its ETA math is unit-testable
//! against a known slope. [`Ceiling`] and its detection are the only parts that
//! touch the OS, and the precedence selection ([`select_ceiling`]) is split out
//! as a pure function so its ordering is testable without a cgroup mounted.

use std::sync::OnceLock;
use std::time::Duration;

/// A least-squares linear fit of a memory series against its sample index.
///
/// `x` is the 0-based sample position in the ring (one step per refresh), `y`
/// is resident memory in KiB. The fit is `y = intercept + slope * x`, so
/// [`slope`](Trend::slope) is KiB gained per refresh — positive means growing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Trend {
    /// KiB gained per sample (per refresh). Negative = shrinking.
    pub slope: f64,
    /// Fitted value at `x = 0`.
    pub intercept: f64,
    /// Number of samples the fit was computed over.
    pub n: usize,
}

impl Trend {
    /// Ordinary-least-squares fit of `mem_kb` against sample index `0..n`.
    ///
    /// `None` for fewer than two samples (a slope needs at least two points) or
    /// a degenerate zero-variance `x` (impossible for distinct indices, guarded
    /// anyway). Hand-rolled rather than pulling a stats crate: the closed-form
    /// slope is three sums, and the durability rule prefers no new dependency.
    pub fn fit(mem_kb: &[u64]) -> Option<Trend> {
        let n = mem_kb.len();
        if n < 2 {
            return None;
        }
        let nf = n as f64;
        let mut sx = 0.0;
        let mut sy = 0.0;
        let mut sxx = 0.0;
        let mut sxy = 0.0;
        for (i, &y) in mem_kb.iter().enumerate() {
            let x = i as f64;
            let y = y as f64;
            sx += x;
            sy += y;
            sxx += x * x;
            sxy += x * y;
        }
        let denom = nf * sxx - sx * sx;
        if denom == 0.0 {
            return None;
        }
        let slope = (nf * sxy - sx * sy) / denom;
        let intercept = (sy - slope * sx) / nf;
        Some(Trend {
            slope,
            intercept,
            n,
        })
    }

    /// The fitted value at the newest sample (`x = n - 1`) — the trend's read of
    /// "current" memory, which smooths a single noisy last sample.
    pub fn current(&self) -> f64 {
        self.intercept + self.slope * (self.n as f64 - 1.0)
    }

    /// Seconds until the fitted line reaches `ceiling` (same units as the fit,
    /// i.e. KiB), given `sample_secs` wall-clock seconds between refreshes.
    ///
    /// `None` when the slope is flat or falling (no exhaustion is coming);
    /// `Some(0.0)` when the current fitted value already sits at/over the
    /// ceiling.
    pub fn eta_secs(&self, ceiling: f64, sample_secs: f64) -> Option<f64> {
        if self.slope <= 0.0 || sample_secs <= 0.0 {
            return None;
        }
        let cur = self.current();
        if cur >= ceiling {
            return Some(0.0);
        }
        let samples_remaining = (ceiling - cur) / self.slope;
        Some(samples_remaining * sample_secs)
    }
}

/// Which real limit the [`Ceiling`] came from, in precedence order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CeilingSource {
    /// Linux cgroup-v2 `memory.max` (the container / slice memory limit).
    CgroupMax,
    /// The process address-space soft limit, `RLIMIT_AS`.
    RlimitAs,
    /// Total machine RAM (no tighter limit is enforced).
    TotalRam,
}

impl CeilingSource {
    /// Short label for the modal ("cgroup", "rlimit", "RAM").
    pub fn label(self) -> &'static str {
        match self {
            CeilingSource::CgroupMax => "cgroup",
            CeilingSource::RlimitAs => "rlimit",
            CeilingSource::TotalRam => "RAM",
        }
    }
}

/// The enforced memory ceiling a projection is measured against.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ceiling {
    /// The limit in bytes.
    pub bytes: u64,
    /// Which enforcement source it was read from.
    pub source: CeilingSource,
}

impl Ceiling {
    /// The process's effective ceiling, detected once and cached for the
    /// session. Reads cgroup-v2 `memory.max` and `RLIMIT_AS` on Linux, falls
    /// back to total RAM everywhere. Cached because the enforced limit does not
    /// change while htoprs runs, and re-reading `/sys`/`/proc` every refresh
    /// would be wasted syscalls.
    pub fn detect() -> Ceiling {
        static CACHE: OnceLock<Ceiling> = OnceLock::new();
        *CACHE.get_or_init(|| {
            select_ceiling(cgroup_memory_max(), rlimit_as(), total_ram_bytes())
                // Absolute last resort: if even total-RAM detection failed,
                // pick a non-zero sentinel so division never blows up. 64 GiB
                // is large enough that a real leak still shows an ETA.
                .unwrap_or(Ceiling {
                    bytes: 64 * 1024 * 1024 * 1024,
                    source: CeilingSource::TotalRam,
                })
        })
    }
}

/// Pure precedence selection over the three ceiling candidates (each already
/// resolved to a concrete byte count, or `None` when absent / unlimited).
/// cgroup wins over rlimit, rlimit over total RAM. Split out from [`Ceiling::detect`]
/// so the ordering is testable without any real limit mounted.
pub fn select_ceiling(
    cgroup_max: Option<u64>,
    rlimit_as: Option<u64>,
    total_ram: Option<u64>,
) -> Option<Ceiling> {
    if let Some(bytes) = cgroup_max.filter(|&b| b > 0) {
        return Some(Ceiling {
            bytes,
            source: CeilingSource::CgroupMax,
        });
    }
    if let Some(bytes) = rlimit_as.filter(|&b| b > 0) {
        return Some(Ceiling {
            bytes,
            source: CeilingSource::RlimitAs,
        });
    }
    total_ram.filter(|&b| b > 0).map(|bytes| Ceiling {
        bytes,
        source: CeilingSource::TotalRam,
    })
}

/// One PID's exhaustion projection, ready to render.
#[derive(Clone, Debug, PartialEq)]
pub struct Projection {
    pub pid: u32,
    pub comm: String,
    /// Growth rate in bytes per second (always positive — only rising trends
    /// produce a projection).
    pub rate_bps: f64,
    /// Seconds until the trend crosses the ceiling.
    pub eta_secs: f64,
    /// The ceiling the projection is measured against.
    pub ceiling_bytes: u64,
    /// Trend-current resident memory in bytes.
    pub cur_bytes: u64,
}

impl Projection {
    /// The projection's ETA as a [`Duration`] (saturating at 0).
    pub fn eta(&self) -> Duration {
        Duration::from_secs_f64(self.eta_secs.max(0.0))
    }
}

/// Build a projection for one PID, or `None` when it is not trending toward
/// exhaustion (too few samples, flat/falling memory, or a non-finite ETA).
///
/// `mem_kb` is the PID's oldest-first resident-memory ring (KiB); `ceiling_bytes`
/// its enforced limit; `sample_secs` the wall-clock seconds between refreshes.
pub fn project(
    pid: u32,
    comm: &str,
    mem_kb: &[u64],
    ceiling_bytes: u64,
    sample_secs: f64,
) -> Option<Projection> {
    let trend = Trend::fit(mem_kb)?;
    let ceiling_kb = ceiling_bytes as f64 / 1024.0;
    let eta_secs = trend.eta_secs(ceiling_kb, sample_secs)?;
    if !eta_secs.is_finite() {
        return None;
    }
    Some(Projection {
        pid,
        comm: comm.to_string(),
        rate_bps: trend.slope * 1024.0 / sample_secs,
        eta_secs,
        ceiling_bytes,
        cur_bytes: (trend.current() * 1024.0).max(0.0) as u64,
    })
}

// ─── formatting ─────────────────────────────────────────────────────────────

/// A byte count as a compact base-1024 string: `8 GB`, `512 MB`, `4 KB`, `900 B`.
pub fn fmt_bytes(bytes: u64) -> String {
    const K: f64 = 1024.0;
    const M: f64 = 1024.0 * 1024.0;
    const G: f64 = 1024.0 * 1024.0 * 1024.0;
    let b = bytes as f64;
    if b >= G {
        format!("{:.1} GB", b / G)
    } else if b >= M {
        format!("{:.0} MB", b / M)
    } else if b >= K {
        format!("{:.0} KB", b / K)
    } else {
        format!("{bytes} B")
    }
}

/// A signed byte-per-second growth rate: `+12 MB/s`, `+340 KB/s`.
pub fn fmt_rate(bps: f64) -> String {
    let sign = if bps < 0.0 { "-" } else { "+" };
    format!("{sign}{}/s", fmt_bytes(bps.abs() as u64))
}

/// A duration as a coarse `~4m20s` / `~2h5m` / `~45s` / `~3d4h` string.
pub fn fmt_eta(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    if s < 60 {
        format!("~{s}s")
    } else if s < 3600 {
        format!("~{}m{}s", s / 60, s % 60)
    } else if s < 86_400 {
        format!("~{}h{}m", s / 3600, (s % 3600) / 60)
    } else {
        format!("~{}d{}h", s / 86_400, (s % 86_400) / 3600)
    }
}

// ─── OS ceiling probes ──────────────────────────────────────────────────────

/// The nearest enforced cgroup-v2 `memory.max` for the current process, walking
/// from its own cgroup leaf up to the root (a limit set on any ancestor slice
/// applies). `None` off Linux, when unified cgroups are not mounted, or when
/// every level is unlimited (`max`).
#[cfg(target_os = "linux")]
fn cgroup_memory_max() -> Option<u64> {
    // cgroup v2 lists a single unified controller as "0::<path>".
    let cg = std::fs::read_to_string("/proc/self/cgroup").ok()?;
    let rel = cg.lines().find_map(|l| l.strip_prefix("0::"))?.trim();
    let mut dir = std::path::PathBuf::from("/sys/fs/cgroup");
    dir.push(rel.trim_start_matches('/'));
    loop {
        if let Ok(raw) = std::fs::read_to_string(dir.join("memory.max")) {
            let v = raw.trim();
            if v != "max" {
                if let Ok(bytes) = v.parse::<u64>() {
                    return Some(bytes);
                }
            }
        }
        // Stop once we have consumed the relative path back to the mount root.
        if !dir.pop() || !dir.starts_with("/sys/fs/cgroup") || dir == std::path::Path::new("/sys/fs/cgroup") {
            // Check the root itself once, then give up.
            if let Ok(raw) = std::fs::read_to_string("/sys/fs/cgroup/memory.max") {
                let v = raw.trim();
                if v != "max" {
                    return v.parse::<u64>().ok();
                }
            }
            return None;
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn cgroup_memory_max() -> Option<u64> {
    None
}

/// The current process's `RLIMIT_AS` soft limit in bytes, or `None` when it is
/// unlimited (`RLIM_INFINITY`) or the call fails. Linux-only: `RLIMIT_AS` is the
/// address-space limit cgroup-less setups use to cap a process.
#[cfg(target_os = "linux")]
fn rlimit_as() -> Option<u64> {
    let mut lim = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    let rc = unsafe { libc::getrlimit(libc::RLIMIT_AS, &mut lim) };
    if rc != 0 || lim.rlim_cur == libc::RLIM_INFINITY {
        return None;
    }
    Some(lim.rlim_cur as u64)
}

#[cfg(not(target_os = "linux"))]
fn rlimit_as() -> Option<u64> {
    None
}

/// Total physical RAM in bytes: `/proc/meminfo` `MemTotal` on Linux,
/// `sysctl hw.memsize` on macOS, `None` elsewhere.
#[cfg(target_os = "linux")]
fn total_ram_bytes() -> Option<u64> {
    let s = std::fs::read_to_string("/proc/meminfo").ok()?;
    for line in s.lines() {
        if let Some(rest) = line.strip_prefix("MemTotal:") {
            let kb: u64 = rest.split_whitespace().next()?.parse().ok()?;
            return Some(kb * 1024);
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn total_ram_bytes() -> Option<u64> {
    let mut size: u64 = 0;
    let mut len = std::mem::size_of::<u64>();
    let name = b"hw.memsize\0";
    let rc = unsafe {
        libc::sysctlbyname(
            name.as_ptr() as *const libc::c_char,
            &mut size as *mut u64 as *mut libc::c_void,
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc == 0 && size > 0 {
        Some(size)
    } else {
        None
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn total_ram_bytes() -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trend_known_slope_gives_known_eta() {
        // A clean +1000 KiB/sample ramp: slope is exactly 1000, current (the
        // fitted newest value) is 4000. With a 10_000 KiB ceiling and 1s
        // refresh, the line needs (10_000-4_000)/1_000 = 6 more samples = 6s.
        let series = [1000u64, 2000, 3000, 4000];
        let t = Trend::fit(&series).expect("fit");
        assert!((t.slope - 1000.0).abs() < 1e-9, "slope was {}", t.slope);
        assert!((t.current() - 4000.0).abs() < 1e-9, "current was {}", t.current());
        let eta = t.eta_secs(10_000.0, 1.0).expect("rising trend has an eta");
        assert!((eta - 6.0).abs() < 1e-9, "eta was {eta}");
    }

    #[test]
    fn trend_scales_eta_with_sample_interval() {
        // Same ramp, 1.5s between refreshes → 6 samples * 1.5s = 9s.
        let t = Trend::fit(&[1000u64, 2000, 3000, 4000]).unwrap();
        let eta = t.eta_secs(10_000.0, 1.5).unwrap();
        assert!((eta - 9.0).abs() < 1e-9, "eta was {eta}");
    }

    #[test]
    fn flat_or_falling_trend_has_no_eta() {
        assert!(Trend::fit(&[5000u64, 5000, 5000]).unwrap().eta_secs(9000.0, 1.0).is_none());
        assert!(Trend::fit(&[4000u64, 3000, 2000]).unwrap().eta_secs(9000.0, 1.0).is_none());
    }

    #[test]
    fn already_over_ceiling_is_zero_eta() {
        let t = Trend::fit(&[1000u64, 2000, 3000, 4000]).unwrap();
        assert_eq!(t.eta_secs(3500.0, 1.0), Some(0.0));
    }

    #[test]
    fn fit_needs_two_points() {
        assert!(Trend::fit(&[]).is_none());
        assert!(Trend::fit(&[42u64]).is_none());
    }

    #[test]
    fn ceiling_precedence_cgroup_beats_rlimit_beats_ram() {
        // All three present → cgroup wins.
        let c = select_ceiling(Some(100), Some(200), Some(300)).unwrap();
        assert_eq!(c.bytes, 100);
        assert_eq!(c.source, CeilingSource::CgroupMax);

        // No cgroup → rlimit wins.
        let c = select_ceiling(None, Some(200), Some(300)).unwrap();
        assert_eq!(c.bytes, 200);
        assert_eq!(c.source, CeilingSource::RlimitAs);

        // Only total RAM → RAM.
        let c = select_ceiling(None, None, Some(300)).unwrap();
        assert_eq!(c.bytes, 300);
        assert_eq!(c.source, CeilingSource::TotalRam);

        // Nothing enforced → no ceiling.
        assert!(select_ceiling(None, None, None).is_none());
    }

    #[test]
    fn ceiling_ignores_zero_valued_sources() {
        // A zero from any probe is treated as absent, not a real limit.
        let c = select_ceiling(Some(0), Some(0), Some(300)).unwrap();
        assert_eq!(c.source, CeilingSource::TotalRam);
    }

    #[test]
    fn project_returns_rising_pid() {
        let p = project(42, "leak", &[1000u64, 2000, 3000, 4000], 10_000 * 1024, 1.0)
            .expect("rising pid projects");
        assert_eq!(p.pid, 42);
        assert!((p.eta_secs - 6.0).abs() < 1e-9);
        // +1000 KiB/sample over 1s = +1024000 bytes/s.
        assert!((p.rate_bps - 1024.0 * 1000.0).abs() < 1e-6);
    }

    #[test]
    fn project_skips_flat_pid() {
        assert!(project(1, "idle", &[2048u64, 2048, 2048], 10_000 * 1024, 1.0).is_none());
    }

    #[test]
    fn format_helpers() {
        assert_eq!(fmt_bytes(8 * 1024 * 1024 * 1024), "8.0 GB");
        assert_eq!(fmt_bytes(512 * 1024 * 1024), "512 MB");
        assert_eq!(fmt_rate(12.0 * 1024.0 * 1024.0), "+12 MB/s");
        assert_eq!(fmt_eta(260.0), "~4m20s");
        assert_eq!(fmt_eta(45.0), "~45s");
    }
}
