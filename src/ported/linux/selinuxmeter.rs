//! Port of `linux/SELinuxMeter.c` — the "SELinux" text meter, which reports
//! whether SELinux is enabled and, if so, whether it is enforcing.
//!
//! SELinux lives entirely on Linux (`/sys/fs/selinux`), so the filesystem
//! probes in `hasSELinuxMount` are `#[cfg(target_os = "linux")]`; on any
//! other host the mount cannot exist and the probe returns `false` (the C is
//! only ever built on Linux — see the module port rules on platform-omitted
//! branches). `statfs`/`statvfs` and `ST_RDONLY` are only referenced inside
//! that Linux-only branch so the module still compiles on macOS.
//!
//! The C keeps two file-scope statics `enabled`/`enforcing` (`SELinuxMeter.c:30`)
//! that `SELinuxMeter_updateValues` writes and `isSelinuxEnforcing` reads;
//! they are modelled here as module `AtomicBool`s.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use std::sync::atomic::{AtomicBool, Ordering};

use crate::ported::linux::compat::Compat_readfile;
use crate::ported::meter::Meter;

/// Port of the file-scope `static bool enabled` / `static bool enforcing`
/// from `SELinuxMeter.c:30`. Written by [`SELinuxMeter_updateValues`] and read
/// by [`isSelinuxEnforcing`].
static enabled: AtomicBool = AtomicBool::new(false);
static enforcing: AtomicBool = AtomicBool::new(false);

/// Port of `static bool hasSELinuxMount(void)` from `SELinuxMeter.c:33`.
/// True only when `/sys/fs/selinux` is mounted with the SELinux magic and is
/// read-write.
#[cfg(target_os = "linux")]
fn hasSELinuxMount() -> bool {
    // struct statfs sfbuf; int r = statfs("/sys/fs/selinux", &sfbuf);
    let mut sfbuf: libc::statfs = unsafe { std::mem::zeroed() };
    let r = unsafe { libc::statfs(c"/sys/fs/selinux".as_ptr(), &mut sfbuf) };
    if r != 0 {
        return false;
    }

    // if ((uint32_t)sfbuf.f_type != /* SELINUX_MAGIC */ 0xf97cff8cU)
    if sfbuf.f_type as u32 != 0xf97cff8c {
        return false;
    }

    // struct statvfs vfsbuf; r = statvfs("/sys/fs/selinux", &vfsbuf);
    let mut vfsbuf: libc::statvfs = unsafe { std::mem::zeroed() };
    let r = unsafe { libc::statvfs(c"/sys/fs/selinux".as_ptr(), &mut vfsbuf) };
    // if (r != 0 || (vfsbuf.f_flag & ST_RDONLY))
    if r != 0 || (vfsbuf.f_flag & libc::ST_RDONLY) != 0 {
        return false;
    }

    true
}

/// Port of `static bool hasSELinuxMount(void)` from `SELinuxMeter.c:33`
/// (non-Linux hosts). `/sys/fs/selinux` is a Linux-only pseudo-filesystem, so
/// there is no mount to find; the C is only compiled on Linux.
#[cfg(not(target_os = "linux"))]
fn hasSELinuxMount() -> bool {
    false
}

/// Port of `static bool isSelinuxEnabled(void)` from `SELinuxMeter.c:53`.
fn isSelinuxEnabled() -> bool {
    // return hasSELinuxMount() && (0 == access("/etc/selinux/config", F_OK));
    hasSELinuxMount() && unsafe { libc::access(c"/etc/selinux/config".as_ptr(), libc::F_OK) } == 0
}

/// Port of `static bool isSelinuxEnforcing(void)` from `SELinuxMeter.c:57`.
/// Reads `/sys/fs/selinux/enforce`; returns whether SELinux is enforcing.
fn isSelinuxEnforcing() -> bool {
    // if (!enabled) return false;
    if !enabled.load(Ordering::Relaxed) {
        return false;
    }

    // char buf[20];
    // ssize_t r = Compat_readfile("/sys/fs/selinux/enforce", buf, sizeof(buf));
    let mut buf = [0u8; 20];
    let r = Compat_readfile(c"/sys/fs/selinux/enforce", &mut buf);
    // if (r < 0) return false;
    if r < 0 {
        return false;
    }

    // int enforce = 0;
    // if (sscanf(buf, "%d", &enforce) != 1) return false;
    // Model sscanf("%d"): skip leading whitespace, optional sign, one or more
    // decimal digits; no digits means no conversion (count != 1).
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    let s = &buf[..end];
    let mut i = 0;
    while i < s.len() && s[i].is_ascii_whitespace() {
        i += 1;
    }
    let neg = i < s.len() && s[i] == b'-';
    if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
        i += 1;
    }
    let digit_start = i;
    let mut enforce: i64 = 0;
    while i < s.len() && s[i].is_ascii_digit() {
        enforce = enforce.wrapping_mul(10).wrapping_add((s[i] - b'0') as i64);
        i += 1;
    }
    if i == digit_start {
        return false;
    }
    if neg {
        enforce = -enforce;
    }

    // return !!enforce;
    enforce != 0
}

/// Port of `static void SELinuxMeter_updateValues(Meter* this)` from
/// `SELinuxMeter.c:75`. Formats `this->txtBuffer` with the enabled/enforcing
/// state.
pub fn SELinuxMeter_updateValues(this: &mut Meter) {
    // enabled = isSelinuxEnabled();
    enabled.store(isSelinuxEnabled(), Ordering::Relaxed);
    // enforcing = isSelinuxEnforcing();
    enforcing.store(isSelinuxEnforcing(), Ordering::Relaxed);

    // xSnprintf(this->txtBuffer, sizeof(this->txtBuffer), "%s%s",
    //    enabled ? "enabled" : "disabled",
    //    enabled ? (enforcing ? "; mode: enforcing" : "; mode: permissive") : "");
    let en = enabled.load(Ordering::Relaxed);
    let enf = enforcing.load(Ordering::Relaxed);
    let mode = if en {
        if enf {
            "; mode: enforcing"
        } else {
            "; mode: permissive"
        }
    } else {
        ""
    };
    this.txtBuffer = format!("{}{}", if en { "enabled" } else { "disabled" }, mode);
}
