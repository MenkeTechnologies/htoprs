//! Port of `linux/CGroupUtils.c` — cgroup path prettifiers.
//!
//! Faithful translation of htop's cgroup-name filters. The C code walks a
//! raw cgroup path with pointer arithmetic and emits a condensed label via
//! an indirected "put character" callback (`StrBuf_putc_t`) used twice: once
//! to count the required length, once to write into the allocated buffer.
//! Here the raw path is carried as a `&str` and pointer walks become byte
//! subslices; the callback is a Rust `fn` pointer.
#![allow(non_snake_case)]
// `StrBuf_state` / `StrBuf_putc_t` mirror C typedef names verbatim (faithful port).
#![allow(non_camel_case_types)]
#![allow(dead_code)]

use crate::ported::xutils::{String_startsWith, String_strchrnul};

// Module-private constants (C `static const char*` at CGroupUtils.c:21-42).
const STR_SLICE_SUFFIX: &str = ".slice";
const STR_SYSTEM_SLICE: &str = "system.slice";
const STR_USER_SLICE: &str = "user.slice";
const STR_MACHINE_SLICE: &str = "machine.slice";
const STR_USER_SLICE_PREFIX: &str = "/user-";
const STR_SYSTEM_SLICE_PREFIX: &str = "/system-";

const STR_LXC_MONITOR_LEGACY: &str = "lxc.monitor";
const STR_LXC_PAYLOAD_LEGACY: &str = "lxc.payload";
const STR_LXC_MONITOR_PREFIX: &str = "lxc.monitor.";
const STR_LXC_PAYLOAD_PREFIX: &str = "lxc.payload.";

const STR_NSPAWN_SCOPE_PREFIX: &str = "machine-";
const STR_NSPAWN_MONITOR_LABEL: &str = "/supervisor";
const STR_NSPAWN_PAYLOAD_LABEL: &str = "/payload";

const STR_SNAP_SCOPE_PREFIX: &str = "snap.";
const STR_POD_SCOPE_PREFIX: &str = "libpod-";
const STR_DOCKER_SCOPE_PREFIX: &str = "docker-";

const STR_SERVICE_SUFFIX: &str = ".service";
const STR_SCOPE_SUFFIX: &str = ".scope";

/// Port of `typedef struct StrBuf_state` from `CGroupUtils.c:44`.
/// During the counting pass `buf` is empty and `size` is 0 (the C `NULL`
/// buffer); during the writing pass `buf` is a zero-filled `size + 1` byte
/// buffer, matching the `xCalloc` allocation.
struct StrBuf_state {
    buf: Vec<u8>,
    size: usize,
    pos: usize,
}

/// Port of `typedef bool (*StrBuf_putc_t)(StrBuf_state* p, char c)` from
/// `CGroupUtils.c:50`.
type StrBuf_putc_t = fn(&mut StrBuf_state, u8) -> bool;

/// Port of `StrBuf_putc_count` from `CGroupUtils.c:52`.
fn StrBuf_putc_count(p: &mut StrBuf_state, _c: u8) -> bool {
    p.pos += 1;
    true
}

/// Port of `StrBuf_putc_write` from `CGroupUtils.c:57`.
fn StrBuf_putc_write(p: &mut StrBuf_state, c: u8) -> bool {
    if p.pos >= p.size {
        return false;
    }

    p.buf[p.pos] = c;
    p.pos += 1;
    true
}

/// Port of `StrBuf_putsn` from `CGroupUtils.c:66`. Writes exactly `count`
/// bytes from `s`.
fn StrBuf_putsn(p: &mut StrBuf_state, w: StrBuf_putc_t, s: &[u8], count: usize) -> bool {
    for i in 0..count {
        if !w(p, s[i]) {
            return false;
        }
    }

    true
}

/// Port of `StrBuf_putsz` from `CGroupUtils.c:74`. Writes the NUL-terminated
/// string `s`; the byte slices passed here carry no interior NUL, so the
/// whole slice is emitted (a `0` byte would terminate as in C).
fn StrBuf_putsz(p: &mut StrBuf_state, w: StrBuf_putc_t, s: &[u8]) -> bool {
    for &c in s {
        if c == 0 {
            break;
        }
        if !w(p, c) {
            return false;
        }
    }

    true
}

/// Port of `Label_checkEqual` from `CGroupUtils.c:82`.
fn Label_checkEqual(labelStart: &str, labelLen: usize, expected: &str) -> bool {
    labelLen == expected.len() && String_startsWith(labelStart, expected)
}

/// Port of `Label_checkPrefix` from `CGroupUtils.c:86`.
fn Label_checkPrefix(labelStart: &str, labelLen: usize, expected: &str) -> bool {
    labelLen > expected.len() && String_startsWith(labelStart, expected)
}

/// Port of `Label_checkSuffix` from `CGroupUtils.c:90`.
fn Label_checkSuffix(labelStart: &str, labelLen: usize, expected: &str) -> bool {
    labelLen > expected.len()
        && String_startsWith(&labelStart[labelLen - expected.len()..], expected)
}

/// Port of `CGroup_filterName_internal` from `CGroupUtils.c:94`.
fn CGroup_filterName_internal(cgroup: &str, s: &mut StrBuf_state, w: StrBuf_putc_t) -> bool {
    let mut cgroup = cgroup;
    while !cgroup.is_empty() {
        if cgroup.as_bytes()[0] == b'/' {
            while !cgroup.is_empty() && cgroup.as_bytes()[0] == b'/' {
                cgroup = &cgroup[1..];
            }

            if !w(s, b'/') {
                return false;
            }

            continue;
        }

        let mut labelStart = cgroup;
        let labelLen = String_strchrnul(labelStart, b'/');
        let mut nextSlash = &labelStart[labelLen..];

        if Label_checkEqual(labelStart, labelLen, STR_SYSTEM_SLICE) {
            cgroup = nextSlash;

            if !StrBuf_putsz(s, w, b"[S]") {
                return false;
            }

            if String_startsWith(cgroup, STR_SYSTEM_SLICE_PREFIX) {
                let idx = String_strchrnul(&cgroup[1..], b'/');
                cgroup = &cgroup[1 + idx..];
                continue;
            }

            continue;
        }

        if Label_checkEqual(labelStart, labelLen, STR_MACHINE_SLICE) {
            cgroup = nextSlash;

            if !StrBuf_putsz(s, w, b"[M]") {
                return false;
            }

            continue;
        }

        if Label_checkEqual(labelStart, labelLen, STR_USER_SLICE) {
            cgroup = nextSlash;

            if !StrBuf_putsz(s, w, b"[U]") {
                return false;
            }

            if !String_startsWith(cgroup, STR_USER_SLICE_PREFIX) {
                continue;
            }

            let prefixLen = STR_USER_SLICE_PREFIX.len();
            let userSliceSlash = prefixLen + String_strchrnul(&cgroup[prefixLen..], b'/');
            let sliceSpec = userSliceSlash - STR_SLICE_SUFFIX.len();

            if !String_startsWith(&cgroup[sliceSpec..], STR_SLICE_SUFFIX) {
                continue;
            }

            let sliceNameLen = sliceSpec - prefixLen;

            s.pos -= 1;
            if !w(s, b':') {
                return false;
            }

            if !StrBuf_putsn(s, w, &cgroup.as_bytes()[prefixLen..], sliceNameLen) {
                return false;
            }

            if !w(s, b']') {
                return false;
            }

            cgroup = &cgroup[userSliceSlash..];

            continue;
        }

        if Label_checkSuffix(labelStart, labelLen, STR_SLICE_SUFFIX) {
            let sliceNameLen = labelLen - STR_SLICE_SUFFIX.len();

            if !w(s, b'[') {
                return false;
            }

            if !StrBuf_putsn(s, w, cgroup.as_bytes(), sliceNameLen) {
                return false;
            }

            if !w(s, b']') {
                return false;
            }

            cgroup = nextSlash;

            continue;
        }

        if Label_checkPrefix(labelStart, labelLen, STR_LXC_PAYLOAD_PREFIX) {
            let prefixLen = STR_LXC_PAYLOAD_PREFIX.len();
            let cgroupNameLen = labelLen - prefixLen;

            if !StrBuf_putsz(s, w, b"[lxc:") {
                return false;
            }

            if !StrBuf_putsn(s, w, &cgroup.as_bytes()[prefixLen..], cgroupNameLen) {
                return false;
            }

            if !w(s, b']') {
                return false;
            }

            cgroup = nextSlash;

            continue;
        }

        if Label_checkPrefix(labelStart, labelLen, STR_LXC_MONITOR_PREFIX) {
            let prefixLen = STR_LXC_MONITOR_PREFIX.len();
            let cgroupNameLen = labelLen - prefixLen;

            if !StrBuf_putsz(s, w, b"[LXC:") {
                return false;
            }

            if !StrBuf_putsn(s, w, &cgroup.as_bytes()[prefixLen..], cgroupNameLen) {
                return false;
            }

            if !w(s, b']') {
                return false;
            }

            cgroup = nextSlash;

            continue;
        }

        // LXC legacy cgroup naming
        if Label_checkEqual(labelStart, labelLen, STR_LXC_MONITOR_LEGACY)
            || Label_checkEqual(labelStart, labelLen, STR_LXC_PAYLOAD_LEGACY)
        {
            let isMonitor = Label_checkEqual(labelStart, labelLen, STR_LXC_MONITOR_LEGACY);

            labelStart = nextSlash;
            while !labelStart.is_empty() && labelStart.as_bytes()[0] == b'/' {
                labelStart = &labelStart[1..];
            }

            let idx = String_strchrnul(labelStart, b'/');
            nextSlash = &labelStart[idx..];
            if idx > 0 {
                if !StrBuf_putsz(s, w, if isMonitor { b"[LXC:" } else { b"[lxc:" }) {
                    return false;
                }

                if !StrBuf_putsn(s, w, labelStart.as_bytes(), idx) {
                    return false;
                }

                if !w(s, b']') {
                    return false;
                }

                cgroup = nextSlash;
                continue;
            }

            labelStart = cgroup;
            nextSlash = &labelStart[labelLen..];
        }

        if Label_checkSuffix(labelStart, labelLen, STR_SERVICE_SUFFIX) {
            let serviceNameLen = labelLen - STR_SERVICE_SUFFIX.len();

            if String_startsWith(cgroup, "user@") {
                cgroup = nextSlash;

                while !cgroup.is_empty() && cgroup.as_bytes()[0] == b'/' {
                    cgroup = &cgroup[1..];
                }

                continue;
            }

            if !StrBuf_putsn(s, w, cgroup.as_bytes(), serviceNameLen) {
                return false;
            }

            cgroup = nextSlash;

            continue;
        }

        if Label_checkSuffix(labelStart, labelLen, STR_SCOPE_SUFFIX) {
            let scopeNameLen = labelLen - STR_SCOPE_SUFFIX.len();

            if Label_checkPrefix(labelStart, scopeNameLen, STR_NSPAWN_SCOPE_PREFIX) {
                let prefixLen = STR_NSPAWN_SCOPE_PREFIX.len();
                let machineScopeNameLen = scopeNameLen - prefixLen;

                let is_monitor = String_startsWith(nextSlash, STR_NSPAWN_MONITOR_LABEL);

                if !StrBuf_putsz(s, w, if is_monitor { b"[SNC:" } else { b"[snc:" }) {
                    return false;
                }

                if !StrBuf_putsn(s, w, &cgroup.as_bytes()[prefixLen..], machineScopeNameLen) {
                    return false;
                }

                if !w(s, b']') {
                    return false;
                }

                cgroup = nextSlash;
                if String_startsWith(nextSlash, STR_NSPAWN_MONITOR_LABEL) {
                    cgroup = &cgroup[STR_NSPAWN_MONITOR_LABEL.len()..];
                } else if String_startsWith(nextSlash, STR_NSPAWN_PAYLOAD_LABEL) {
                    cgroup = &cgroup[STR_NSPAWN_PAYLOAD_LABEL.len()..];
                }

                continue;
            } else if Label_checkPrefix(labelStart, scopeNameLen, STR_SNAP_SCOPE_PREFIX) {
                let prefixLen = STR_SNAP_SCOPE_PREFIX.len();
                let mut nextDot = prefixLen + String_strchrnul(&labelStart[prefixLen..], b'.');

                if !StrBuf_putsz(s, w, b"!snap:") {
                    return false;
                }

                if nextDot >= scopeNameLen {
                    nextDot = scopeNameLen;
                }

                if !StrBuf_putsn(
                    s,
                    w,
                    &labelStart.as_bytes()[prefixLen..],
                    nextDot - prefixLen,
                ) {
                    return false;
                }

                cgroup = nextSlash;

                continue;
            } else if Label_checkPrefix(labelStart, scopeNameLen, STR_POD_SCOPE_PREFIX) {
                let prefixLen = STR_POD_SCOPE_PREFIX.len();
                let mut nextDot = prefixLen + String_strchrnul(&labelStart[prefixLen..], b'.');

                if !StrBuf_putsz(s, w, b"!pod:") {
                    return false;
                }

                if nextDot >= scopeNameLen {
                    nextDot = scopeNameLen;
                }

                if !StrBuf_putsn(
                    s,
                    w,
                    &labelStart.as_bytes()[prefixLen..],
                    (nextDot - prefixLen).min(12),
                ) {
                    return false;
                }

                cgroup = nextSlash;

                continue;
            } else if Label_checkPrefix(labelStart, scopeNameLen, STR_DOCKER_SCOPE_PREFIX) {
                let prefixLen = STR_DOCKER_SCOPE_PREFIX.len();
                let mut nextDot = prefixLen + String_strchrnul(&labelStart[prefixLen..], b'.');

                if !StrBuf_putsz(s, w, b"!docker:") {
                    return false;
                }

                if nextDot >= scopeNameLen {
                    nextDot = scopeNameLen;
                }

                if !StrBuf_putsn(
                    s,
                    w,
                    &labelStart.as_bytes()[prefixLen..],
                    (nextDot - prefixLen).min(12),
                ) {
                    return false;
                }

                cgroup = nextSlash;

                continue;
            }

            if !w(s, b'!') {
                return false;
            }

            if !StrBuf_putsn(s, w, cgroup.as_bytes(), scopeNameLen) {
                return false;
            }

            cgroup = nextSlash;

            continue;
        }

        // Default behavior: Copy the full label
        cgroup = labelStart;

        if !StrBuf_putsn(s, w, cgroup.as_bytes(), labelLen) {
            return false;
        }

        cgroup = nextSlash;
    }

    true
}

/// Port of `CGroup_filterName` from `CGroupUtils.c:363`. Returns `None` for
/// the C `NULL` (a `w` callback failing mid-walk).
pub fn CGroup_filterName(cgroup: &str) -> Option<String> {
    let mut s = StrBuf_state {
        buf: Vec::new(),
        size: 0,
        pos: 0,
    };

    if !CGroup_filterName_internal(cgroup, &mut s, StrBuf_putc_count) {
        return None;
    }

    s.buf = vec![0u8; s.pos + 1];
    s.size = s.pos;
    s.pos = 0;

    if !CGroup_filterName_internal(cgroup, &mut s, StrBuf_putc_write) {
        return None;
    }

    s.buf[s.size] = b'\0';
    Some(String::from_utf8_lossy(&s.buf[..s.size]).into_owned())
}

/// Port of `CGroup_filterContainer_internal` from `CGroupUtils.c:387`.
fn CGroup_filterContainer_internal(cgroup: &str, s: &mut StrBuf_state, w: StrBuf_putc_t) -> bool {
    let mut cgroup = cgroup;
    while !cgroup.is_empty() {
        if cgroup.as_bytes()[0] == b'/' {
            while !cgroup.is_empty() && cgroup.as_bytes()[0] == b'/' {
                cgroup = &cgroup[1..];
            }

            continue;
        }

        let mut labelStart = cgroup;
        let labelLen = String_strchrnul(labelStart, b'/');
        let mut nextSlash = &labelStart[labelLen..];

        if Label_checkPrefix(labelStart, labelLen, STR_LXC_PAYLOAD_PREFIX) {
            let prefixLen = STR_LXC_PAYLOAD_PREFIX.len();
            let cgroupNameLen = labelLen - prefixLen;

            if !StrBuf_putsz(s, w, b"/lxc:") {
                return false;
            }

            if !StrBuf_putsn(s, w, &cgroup.as_bytes()[prefixLen..], cgroupNameLen) {
                return false;
            }

            cgroup = nextSlash;

            continue;
        }

        // LXC legacy cgroup naming
        if Label_checkEqual(labelStart, labelLen, STR_LXC_PAYLOAD_LEGACY) {
            labelStart = nextSlash;
            while !labelStart.is_empty() && labelStart.as_bytes()[0] == b'/' {
                labelStart = &labelStart[1..];
            }

            let idx = String_strchrnul(labelStart, b'/');
            nextSlash = &labelStart[idx..];
            if idx > 0 {
                if !StrBuf_putsz(s, w, b"/lxc:") {
                    return false;
                }

                if !StrBuf_putsn(s, w, labelStart.as_bytes(), idx) {
                    return false;
                }

                cgroup = nextSlash;
                continue;
            }

            labelStart = cgroup;
            nextSlash = &labelStart[labelLen..];
        }

        if Label_checkSuffix(labelStart, labelLen, STR_SCOPE_SUFFIX) {
            let scopeNameLen = labelLen - STR_SCOPE_SUFFIX.len();

            if Label_checkPrefix(labelStart, scopeNameLen, STR_NSPAWN_SCOPE_PREFIX) {
                let prefixLen = STR_NSPAWN_SCOPE_PREFIX.len();
                let machineScopeNameLen = scopeNameLen - prefixLen;

                let is_monitor = String_startsWith(nextSlash, STR_NSPAWN_MONITOR_LABEL);

                if !is_monitor {
                    if !StrBuf_putsz(s, w, b"/snc:") {
                        return false;
                    }

                    if !StrBuf_putsn(s, w, &cgroup.as_bytes()[prefixLen..], machineScopeNameLen) {
                        return false;
                    }
                }

                cgroup = nextSlash;
                if String_startsWith(nextSlash, STR_NSPAWN_MONITOR_LABEL) {
                    cgroup = &cgroup[STR_NSPAWN_MONITOR_LABEL.len()..];
                } else if String_startsWith(nextSlash, STR_NSPAWN_PAYLOAD_LABEL) {
                    cgroup = &cgroup[STR_NSPAWN_PAYLOAD_LABEL.len()..];
                }

                continue;
            } else if Label_checkPrefix(labelStart, scopeNameLen, STR_POD_SCOPE_PREFIX) {
                let prefixLen = STR_POD_SCOPE_PREFIX.len();
                let mut nextDot = prefixLen + String_strchrnul(&labelStart[prefixLen..], b'.');

                if !StrBuf_putsz(s, w, b"/pod:") {
                    return false;
                }

                if nextDot >= scopeNameLen {
                    nextDot = scopeNameLen;
                }

                if !StrBuf_putsn(
                    s,
                    w,
                    &labelStart.as_bytes()[prefixLen..],
                    (nextDot - prefixLen).min(12),
                ) {
                    return false;
                }

                cgroup = nextSlash;

                continue;
            } else if Label_checkPrefix(labelStart, scopeNameLen, STR_DOCKER_SCOPE_PREFIX) {
                let prefixLen = STR_DOCKER_SCOPE_PREFIX.len();
                let mut nextDot = prefixLen + String_strchrnul(&labelStart[prefixLen..], b'.');

                if !StrBuf_putsz(s, w, b"!docker:") {
                    return false;
                }

                if nextDot >= scopeNameLen {
                    nextDot = scopeNameLen;
                }

                if !StrBuf_putsn(
                    s,
                    w,
                    &labelStart.as_bytes()[prefixLen..],
                    (nextDot - prefixLen).min(12),
                ) {
                    return false;
                }

                cgroup = nextSlash;

                continue;
            }

            cgroup = nextSlash;

            continue;
        }

        cgroup = nextSlash;
    }

    true
}

/// Port of `CGroup_filterContainer` from `CGroupUtils.c:506`. Returns `"/"`
/// when nothing was emitted, mirroring the C `xStrdup("/")` fast path.
pub fn CGroup_filterContainer(cgroup: &str) -> Option<String> {
    let mut s = StrBuf_state {
        buf: Vec::new(),
        size: 0,
        pos: 0,
    };

    if !CGroup_filterContainer_internal(cgroup, &mut s, StrBuf_putc_count) {
        return None;
    }

    if s.pos == 0 {
        return Some(String::from("/"));
    }

    s.buf = vec![0u8; s.pos + 1];
    s.size = s.pos;
    s.pos = 0;

    if !CGroup_filterContainer_internal(cgroup, &mut s, StrBuf_putc_write) {
        return None;
    }

    s.buf[s.size] = b'\0';
    Some(String::from_utf8_lossy(&s.buf[..s.size]).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_name_passthrough_simple() {
        assert_eq!(CGroup_filterName("foo/bar").as_deref(), Some("foo/bar"));
    }

    #[test]
    fn filter_name_system_slice() {
        assert_eq!(
            CGroup_filterName("system.slice/cron.service").as_deref(),
            Some("[S]/cron")
        );
    }

    #[test]
    fn filter_name_generic_slice() {
        assert_eq!(
            CGroup_filterName("foo.slice/bar.service").as_deref(),
            Some("[foo]/bar")
        );
    }

    #[test]
    fn filter_name_lxc_payload_prefix() {
        assert_eq!(
            CGroup_filterName("lxc.payload.mycontainer").as_deref(),
            Some("[lxc:mycontainer]")
        );
    }

    #[test]
    fn filter_name_scope_default() {
        assert_eq!(
            CGroup_filterName("session-c1.scope").as_deref(),
            Some("!session-c1")
        );
    }

    #[test]
    fn filter_container_empty_is_root() {
        assert_eq!(CGroup_filterContainer("system.slice").as_deref(), Some("/"));
    }

    #[test]
    fn filter_container_lxc_payload() {
        assert_eq!(
            CGroup_filterContainer("lxc.payload.web").as_deref(),
            Some("/lxc:web")
        );
    }

    #[test]
    fn filter_container_docker_scope() {
        assert_eq!(
            CGroup_filterContainer("docker-abcdef0123456789.scope").as_deref(),
            Some("!docker:abcdef012345")
        );
    }
}
