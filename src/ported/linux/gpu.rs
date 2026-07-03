//! Port of `linux/GPU.c` — per-process GPU busy-time accounting parsed
//! from each task's `fdinfo/*` DRM usage-stats entries.
//!
//! This port commits to the modern-Linux `HAVE_OPENAT` build variant (the
//! `#ifndef HAVE_OPENAT` fallback that rebuilds the `/proc/<pid>/fdinfo`
//! path by hand is the other build variant and is intentionally not ported;
//! see the module port rules). The `Machine* -> LinuxMachine*` upcast in
//! `update_machine_gpu` faithfully mirrors the C `(LinuxMachine*) host`
//! cast: `Table::host` is the same opaque back-reference the C threads
//! through the tree, and the concrete host is always a `LinuxMachine` whose
//! embedded `Machine super` is its first field.
//!
//! Documentation reference:
//! <https://www.kernel.org/doc/html/latest/gpu/drm-usage-stats.html>
#![allow(non_snake_case)]
#![allow(non_camel_case_types)]
#![allow(dead_code)]

use std::ffi::{CStr, CString};

use libc::{c_char, c_int};

use crate::ported::linux::compat::{openat_arg_t, Compat_openat, Compat_readfileat};
use crate::ported::linux::linuxmachine::{GPUEngineData, LinuxMachine};
use crate::ported::linux::linuxprocess::LinuxProcess;
use crate::ported::linux::linuxprocesstable::LinuxProcessTable;
use crate::ported::machine::Machine;
use crate::ported::xutils::{String_eq_nullable, String_startsWith};

/// Port of `typedef unsigned long long int ClientID` (`GPU.c:22`).
type ClientID = u64;

/// Port of `#define INVALID_CLIENT_ID ((ClientID)-1)` (`GPU.c:23`).
const INVALID_CLIENT_ID: ClientID = ClientID::MAX;

/// Port of `typedef struct ClientInfo_` (`GPU.c:26`). A singly-linked list
/// of the DRM clients already accounted for this scan; `next` is the C
/// `struct ClientInfo_*` link.
struct ClientInfo {
    /// C `char* pdev` — parent-device string (`None` models `NULL`).
    pdev: Option<String>,
    /// C `ClientID id`.
    id: ClientID,
    next: Option<Box<ClientInfo>>,
}

/// Port of `enum section_state` (`GPU.c:32`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum section_state {
    SECST_UNKNOWN,
    SECST_DUPLICATE,
    SECST_NEW,
}

/// Port of `GPU.c:38`.
///
/// C `const ClientInfo* parsed` is modeled as the head of the list
/// (`Option<&ClientInfo>`); `const char* pdev` as `Option<&str>`.
fn is_duplicate_client(mut parsed: Option<&ClientInfo>, id: ClientID, pdev: Option<&str>) -> bool {
    while let Some(node) = parsed {
        if id == node.id && String_eq_nullable(pdev, node.pdev.as_deref()) {
            return true;
        }
        parsed = node.next.as_deref();
    }

    false
}

/// Port of `GPU.c:48`.
///
/// The C `const char* engine` + `size_t engine_len` pair (a pointer into the
/// fdinfo line with an explicit length) is collapsed into a single exact
/// engine-name slice `engine`, so the C `strncmp(key, engine, engine_len) ==
/// 0 && key[engine_len] == '\0'` key match becomes a whole-string equality.
fn update_machine_gpu(lpt: &mut LinuxProcessTable, time: u64, engine: &str) {
    // C: Machine* host = lpt->super.super.host;
    //    LinuxMachine* lhost = (LinuxMachine*) host;
    let host = lpt.super_.super_.host;
    debug_assert!(!host.is_null());
    let lhost: &mut LinuxMachine = unsafe { &mut *(host as *mut Machine as *mut LinuxMachine) };

    // C: GPUEngineData** engineData = &lhost->gpuEngineData;
    //    while (*engineData) { if key matches break; engineData = &(*engineData)->next; }
    {
        let mut engineData = &mut lhost.gpuEngineData;
        loop {
            match engineData {
                None => break,
                Some(node) if node.key.as_deref() == Some(engine) => break,
                Some(_) => {}
            }
            engineData = &mut engineData.as_mut().unwrap().next;
        }

        // C: if (!*engineData) { *engineData = xMalloc(...); ... }
        if engineData.is_none() {
            *engineData = Some(Box::new(GPUEngineData {
                prevTime: 0,
                curTime: 0,
                key: Some(engine.to_string()),
                next: None,
            }));
        }

        // C: (*engineData)->curTime += time;
        engineData.as_mut().unwrap().curTime += time;
    }

    // C: lhost->curGpuTime += time;
    lhost.curGpuTime += time;
}

/// Port of `GPU.c:80`.
pub fn GPU_readProcessData(
    lpt: &mut LinuxProcessTable,
    lp: &mut LinuxProcess,
    procFd: openat_arg_t,
) {
    // C: const Machine* host = lp->super.super.host;
    let host = lp.super_.super_.host as *const Machine;
    let mut parsed_ids: Option<Box<ClientInfo>> = None;
    let mut new_gpu_time: u64 = 0;

    let host_monotonicMs = unsafe { (*host).monotonicMs };
    let host_prevMonotonicMs = unsafe { (*host).prevMonotonicMs };

    /* check only if active in last check or last scan was more than 5s ago */
    if lp.gpu_activityMs != 0 && host_monotonicMs - lp.gpu_activityMs < 5000 {
        lp.gpu_percent = 0.0;
        return;
    }
    lp.gpu_activityMs = host_monotonicMs;

    // Faithful reimplementation of the C `strtoull(s, &endptr, 10)` pattern:
    // returns (value, bytes consumed before endptr, errno-was-set). Used for
    // both the client-id and the per-engine time fields.
    // C `errno` access, portable across glibc/musl (`__errno_location`) and
    // the BSD/macOS libc (`__error`), matching `linux/Compat.c`'s port.
    let errno_location = || -> *mut c_int {
        #[cfg(any(target_os = "linux", target_os = "android"))]
        {
            unsafe { libc::__errno_location() }
        }
        #[cfg(any(target_os = "netbsd", target_os = "openbsd"))]
        {
            unsafe { libc::__errno() }
        }
        #[cfg(not(any(
            target_os = "linux",
            target_os = "android",
            target_os = "netbsd",
            target_os = "openbsd"
        )))]
        {
            unsafe { libc::__error() }
        }
    };
    let strtoull_c = |s: &str| -> (u64, usize, bool) {
        let cs = CString::new(s).unwrap_or_default();
        unsafe {
            *errno_location() = 0;
            let mut endptr: *mut c_char = std::ptr::null_mut();
            let val = libc::strtoull(cs.as_ptr(), &mut endptr, 10);
            let err = *errno_location();
            let consumed = endptr.offset_from(cs.as_ptr()) as usize;
            (val, consumed, err != 0)
        }
    };

    // C initializes `fdinfoFd = -1`, but the labeled block below assigns it
    // unconditionally before any use; declare uninitialized to avoid a dead store.
    let mut fdinfoFd: c_int;
    let mut fdinfoDir: *mut libc::DIR = std::ptr::null_mut();

    'out: {
        fdinfoFd = Compat_openat(
            procFd,
            c"fdinfo",
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_DIRECTORY | libc::O_CLOEXEC,
        );
        if fdinfoFd == -1 {
            break 'out;
        }

        fdinfoDir = unsafe { libc::fdopendir(fdinfoFd) };
        if fdinfoDir.is_null() {
            break 'out;
        }
        fdinfoFd = -1;

        loop {
            let mut pdev: Option<String> = None;
            let mut client_id: ClientID = INVALID_CLIENT_ID;
            let mut sstate = section_state::SECST_UNKNOWN;

            let entry = unsafe { libc::readdir(fdinfoDir) };
            if entry.is_null() {
                break;
            }
            let ename = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };

            // C: if (ename[0] == '.' && (ename[1] == '\0' || (ename[1] == '.' && ename[2] == '\0'))) continue;
            if ename.to_bytes() == b"." || ename.to_bytes() == b".." {
                continue;
            }

            // C (HAVE_OPENAT): Compat_readfileat(dirfd(fdinfoDir), ename, buffer, sizeof(buffer));
            let mut buffer = [0u8; 4096];
            let ret = Compat_readfileat(unsafe { libc::dirfd(fdinfoDir) }, ename, &mut buffer);
            /* eventfd information can be huge */
            if ret <= 0 || (ret as usize) >= buffer.len() - 1 {
                continue;
            }

            let content = &buffer[..ret as usize];
            for line_bytes in content.split(|&b| b == b'\n') {
                let line_cow = String::from_utf8_lossy(line_bytes);
                let line: &str = &line_cow;

                if !String_startsWith(line, "drm-") {
                    continue;
                }
                let line = &line["drm-".len()..];

                if line.starts_with('c') && String_startsWith(line, "client-id:") {
                    if sstate == section_state::SECST_NEW {
                        debug_assert!(client_id != INVALID_CLIENT_ID);

                        parsed_ids = Some(Box::new(ClientInfo {
                            id: client_id,
                            pdev: pdev.take(),
                            next: parsed_ids.take(),
                        }));
                    }

                    sstate = section_state::SECST_UNKNOWN;

                    let rest = &line["client-id:".len()..];
                    let (val, consumed, err) = strtoull_c(rest);
                    client_id = if err || consumed != rest.len() {
                        INVALID_CLIENT_ID
                    } else {
                        val
                    };
                } else if line.starts_with('p') && String_startsWith(line, "pdev:") {
                    let p = &line["pdev:".len()..];

                    // C: while (isspace((unsigned char)*p)) p++;
                    let pb = p.as_bytes();
                    let mut i = 0;
                    while i < pb.len()
                        && matches!(pb[i], b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
                    {
                        i += 1;
                    }
                    let p = &p[i..];

                    debug_assert!(pdev.is_none() || pdev.as_deref() == Some(p));
                    if pdev.is_none() {
                        pdev = Some(p.to_string());
                    }
                } else if line.starts_with('e') && String_startsWith(line, "engine-") {
                    if sstate == section_state::SECST_DUPLICATE {
                        continue;
                    }

                    let engineStart = &line["engine-".len()..];

                    if String_startsWith(engineStart, "capacity-") {
                        continue;
                    }

                    // C: const char* delim = strchr(line, ':');
                    let delim = match line.find(':') {
                        Some(d) => d,
                        None => continue,
                    };

                    let after = &line[delim + 1..];
                    let (value, consumed, err) = strtoull_c(after);
                    if !err && String_startsWith(&after[consumed..], " ns") {
                        if sstate == section_state::SECST_UNKNOWN {
                            if client_id != INVALID_CLIENT_ID
                                && !is_duplicate_client(
                                    parsed_ids.as_deref(),
                                    client_id,
                                    pdev.as_deref(),
                                )
                            {
                                sstate = section_state::SECST_NEW;
                            } else {
                                sstate = section_state::SECST_DUPLICATE;
                            }
                        }

                        if sstate == section_state::SECST_NEW {
                            new_gpu_time += value;
                            // C: update_machine_gpu(lpt, value, engineStart, delim - engineStart);
                            let engine = &engineStart[..delim - "engine-".len()];
                            update_machine_gpu(lpt, value, engine);
                        }
                    }
                }
            } /* finished parsing lines */

            if sstate == section_state::SECST_NEW {
                debug_assert!(client_id != INVALID_CLIENT_ID);

                parsed_ids = Some(Box::new(ClientInfo {
                    id: client_id,
                    pdev: pdev.take(),
                    next: parsed_ids.take(),
                }));
            }

            // C: free(pdev); — `pdev` is dropped at the end of this iteration.
        } /* finished parsing fdinfo entries */

        if new_gpu_time > 0 {
            let gputimeDelta = new_gpu_time.saturating_sub(lp.gpu_time);
            let monotonicTimeDelta = host_monotonicMs - host_prevMonotonicMs;
            lp.gpu_percent =
                100.0f32 * gputimeDelta as f32 / (1000.0 * 1000.0) / monotonicTimeDelta as f32;

            lp.gpu_activityMs = 0;
        } else {
            lp.gpu_percent = 0.0;
        }
    }

    // out:
    lp.gpu_time = new_gpu_time;

    // C: while (parsed_ids) { ... free(parsed_ids->pdev); free(parsed_ids); ... }
    drop(parsed_ids);

    if !fdinfoDir.is_null() {
        unsafe {
            libc::closedir(fdinfoDir);
        }
    }
    if fdinfoFd != -1 {
        unsafe {
            libc::close(fdinfoFd);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_duplicate_client_matches_id_and_pdev() {
        let list = ClientInfo {
            id: 7,
            pdev: Some("0000:01:00.0".to_string()),
            next: Some(Box::new(ClientInfo {
                id: 3,
                pdev: None,
                next: None,
            })),
        };

        // exact id + pdev match
        assert!(is_duplicate_client(Some(&list), 7, Some("0000:01:00.0")));
        // matching id, differing pdev -> not a duplicate
        assert!(!is_duplicate_client(Some(&list), 7, None));
        // both-NULL pdev match on the tail node
        assert!(is_duplicate_client(Some(&list), 3, None));
        // unknown id
        assert!(!is_duplicate_client(Some(&list), 9, None));
        // empty list
        assert!(!is_duplicate_client(None, 7, Some("0000:01:00.0")));
    }
}
