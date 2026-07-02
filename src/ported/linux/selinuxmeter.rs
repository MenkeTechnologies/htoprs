//! Port of `linux/SELinuxMeter.c` — the "SELinux" text meter, which reports
//! whether SELinux is enabled and, if so, its enforcing mode.
//!
//! SELinux lives entirely on Linux (`/sys/fs/selinux`), so the filesystem
//! probes in [`hasSELinuxMount`] are `#[cfg(target_os = "linux")]`; on any
//! other host the mount cannot exist and the probe returns `false` (the C is
//! only ever built on Linux — see the module port rules on platform-omitted
//! branches). `statfs`/`statvfs` and `ST_RDONLY` are only referenced inside
//! that Linux-only branch so the module still compiles on macOS.
//!
//! Version note: this vendored SPEC renamed the old `isSelinuxEnforcing()`
//! (a `bool` predicate) to `getSelinuxEnforcing()` returning an
//! `EnforcingMode` enum (vendor/htop commit "improve SELinux detection in
//! constrained environments"). The port-purity name snapshot
//! (`tests/data/htop_c_fn_names.txt`) predates that rename and still lists
//! `isSelinuxEnforcing`, not `getSelinuxEnforcing`, so the new name is not a
//! legal free-fn name here. `getSelinuxEnforcing` is therefore ported as a
//! local **closure** inside [`SELinuxMeter_updateValues`] (rule 1 endorses
//! closures for the C's own static helpers); [`isSelinuxEnforcing`] stays a
//! documented stub since the SPEC no longer contains a function by that name.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use libc::ssize_t;

use crate::ported::linux::compat::Compat_readfile;
use crate::ported::meter::Meter;

/// Port of `typedef enum { ... } EnforcingMode` from `SELinuxMeter.c:31`.
/// Discriminants match the C enumeration order and index [`enforcingText`].
#[derive(Clone, Copy, PartialEq, Eq)]
enum EnforcingMode {
    Permissive = 0,
    Enforcing = 1,
    Unknown = 2,
    Disabled = 3,
}

/// Port of `static const char* const enforcingText[]` from `SELinuxMeter.c:38`.
/// Indexed by [`EnforcingMode`] (designated initializers in C order).
const enforcingText: [&str; 4] = [
    "enabled; mode: permissive",
    "enabled; mode: enforcing",
    "enabled; mode: unknown",
    "disabled",
];

/// Port of `static bool hasSELinuxMount(void)` from `SELinuxMeter.c:45`.
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

/// Port of `static bool hasSELinuxMount(void)` from `SELinuxMeter.c:45`
/// (non-Linux hosts). `/sys/fs/selinux` is a Linux-only pseudo-filesystem, so
/// there is no mount to find; the C is only compiled on Linux.
#[cfg(not(target_os = "linux"))]
fn hasSELinuxMount() -> bool {
    false
}

/// Port of `static bool isSelinuxEnabled(void)` from `SELinuxMeter.c:65`.
fn isSelinuxEnabled() -> bool {
    hasSELinuxMount()
}

/// TODO: not a faithful stub of live C — the vendored SPEC no longer contains
/// a function named `isSelinuxEnforcing`. The commit "improve SELinux
/// detection in constrained environments" renamed it to `getSelinuxEnforcing`
/// (now returning an `EnforcingMode` enum, `SELinuxMeter.c:69`), which is not
/// a legal free-fn name here (the port-purity snapshot still lists the old
/// name). The renamed logic is ported as a closure inside
/// [`SELinuxMeter_updateValues`]; this slot is retained only because the stale
/// name snapshot names it, and is left unimplemented.
pub fn isSelinuxEnforcing() {
    todo!("SELinuxMeter.c: renamed to getSelinuxEnforcing (ported as a closure in SELinuxMeter_updateValues)")
}

/// Port of `static void SELinuxMeter_updateValues(Meter* this)` from
/// `SELinuxMeter.c:85`. Formats `this->txtBuffer` with the enforcing text for
/// the current mode.
///
/// The static helper `getSelinuxEnforcing()` (`SELinuxMeter.c:69`) is ported
/// as the `get_selinux_enforcing` closure below — see the module docs for why
/// it cannot be a free fn under the current name snapshot.
pub fn SELinuxMeter_updateValues(this: &mut Meter) {
    // static EnforcingMode getSelinuxEnforcing(void)
    let get_selinux_enforcing = || -> EnforcingMode {
        // if (!isSelinuxEnabled()) return SELINUX_DISABLED;
        if !isSelinuxEnabled() {
            return EnforcingMode::Disabled;
        }

        // char buf[20];
        // ssize_t r = Compat_readfile("/sys/fs/selinux/enforce", buf, sizeof(buf));
        let mut buf = [0u8; 20];
        let r = Compat_readfile(c"/sys/fs/selinux/enforce", &mut buf);
        if r < 0 {
            // return (r == -ENOENT) ? SELINUX_DISABLED : SELINUX_UNKNOWN;
            return if r == -(libc::ENOENT as ssize_t) {
                EnforcingMode::Disabled
            } else {
                EnforcingMode::Unknown
            };
        }

        // int enforce = 0;
        // if (sscanf(buf, "%d", &enforce) != 1) return SELINUX_UNKNOWN;
        // Model sscanf("%d"): skip leading whitespace, optional sign, one or
        // more decimal digits; no digits means no conversion (count != 1).
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
            return EnforcingMode::Unknown;
        }
        if neg {
            enforce = -enforce;
        }

        // return enforce ? SELINUX_ENFORCING : SELINUX_PERMISSIVE;
        if enforce != 0 {
            EnforcingMode::Enforcing
        } else {
            EnforcingMode::Permissive
        }
    };

    // EnforcingMode enforcing = getSelinuxEnforcing();
    let enforcing = get_selinux_enforcing();
    // xSnprintf(this->txtBuffer, sizeof(this->txtBuffer), "%s", enforcingText[enforcing]);
    this.txtBuffer = enforcingText[enforcing as usize].to_string();
}
