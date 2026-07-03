//! Port of `darwin/PlatformHelpers.c` — kernel-version / CPU-brand /
//! Rosetta-translation probes built on `sysctlbyname`.
//!
//! Self-contained: only `libc` (`sysctlbyname`, `errno`) and the
//! already-ported `String_safeStrncpy` are needed. The C
//! function-local `static KernelVersion cachedKernelVersion` is modeled
//! as a module-level `Mutex<KernelVersion>` under its C name.
//!
//! htop prints its `WARN:` diagnostics to `stderr`; this port keeps that
//! behaviour verbatim (`eprintln!`) so output matches the C original.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use std::mem::size_of;
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;
use std::sync::Mutex;

use crate::ported::xutils::String_safeStrncpy;

/// Port of `typedef struct KernelVersion` (`darwin/PlatformHelpers.h:17`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct KernelVersion {
    pub major: i16,
    pub minor: i16,
    pub patch: i16,
}

/// The C function-local `static KernelVersion cachedKernelVersion`
/// (`darwin/PlatformHelpers.c:24`).
static cachedKernelVersion: Mutex<KernelVersion> = Mutex::new(KernelVersion {
    major: 0,
    minor: 0,
    patch: 0,
});

/// Port of `void Platform_GetKernelVersion(KernelVersion* k)`
/// (`darwin/PlatformHelpers.c:23`).
pub fn Platform_GetKernelVersion(k: &mut KernelVersion) {
    let mut cached = cachedKernelVersion.lock().unwrap();

    if cached.major == 0 {
        // just in case it fails someday
        *cached = KernelVersion {
            major: -1,
            minor: -1,
            patch: -1,
        };
        let mut str = [0u8; 256];
        let mut size = size_of::<[u8; 256]>();
        let ret = unsafe {
            libc::sysctlbyname(
                b"kern.osrelease\0".as_ptr() as *const c_char,
                str.as_mut_ptr() as *mut c_void,
                &mut size,
                ptr::null_mut(),
                0,
            )
        };
        if ret == 0 {
            // sscanf(str, "%hd.%hd.%hd", &major, &minor, &patch): fill as
            // many fields as parse; leave the rest at their -1 preset.
            let end = str.iter().position(|&b| b == 0).unwrap_or(str.len());
            let text = &str[..end];
            let fields = [
                ptr::addr_of_mut!(cached.major),
                ptr::addr_of_mut!(cached.minor),
                ptr::addr_of_mut!(cached.patch),
            ];
            let mut pos = 0;
            for f in 0..3 {
                if f > 0 {
                    if pos >= text.len() || text[pos] != b'.' {
                        break;
                    }
                    pos += 1;
                }
                let neg = pos < text.len() && text[pos] == b'-';
                if neg {
                    pos += 1;
                }
                let dstart = pos;
                let mut val: i32 = 0;
                while pos < text.len() && text[pos].is_ascii_digit() {
                    val = val * 10 + (text[pos] - b'0') as i32;
                    pos += 1;
                }
                if pos == dstart {
                    break;
                }
                if neg {
                    val = -val;
                }
                unsafe {
                    *fields[f] = val as i16;
                }
            }
        }
    }

    *k = *cached;
}

/// Port of `int Platform_CompareKernelVersion(KernelVersion v)`
/// (`darwin/PlatformHelpers.c:39`).
pub fn Platform_CompareKernelVersion(v: KernelVersion) -> c_int {
    let mut actualVersion = KernelVersion::default();
    Platform_GetKernelVersion(&mut actualVersion);

    if actualVersion.major != v.major {
        return (actualVersion.major - v.major) as c_int;
    }
    if actualVersion.minor != v.minor {
        return (actualVersion.minor - v.minor) as c_int;
    }
    if actualVersion.patch != v.patch {
        return (actualVersion.patch - v.patch) as c_int;
    }

    0
}

/// Port of `bool Platform_KernelVersionIsBetween(KernelVersion lowerBound,
/// KernelVersion upperBound)` (`darwin/PlatformHelpers.c:56`).
pub fn Platform_KernelVersionIsBetween(
    lowerBound: KernelVersion,
    upperBound: KernelVersion,
) -> bool {
    0 <= Platform_CompareKernelVersion(lowerBound) && Platform_CompareKernelVersion(upperBound) < 0
}

/// Port of `void Platform_getCPUBrandString(char* cpuBrandString, size_t
/// cpuBrandStringSize)` (`darwin/PlatformHelpers.c:61`). The buffer is
/// modeled as a `&mut [u8]` carrying both the C pointer and its size.
pub fn Platform_getCPUBrandString(cpuBrandString: &mut [u8]) {
    let mut cpuBrandStringSize = cpuBrandString.len();
    let ret = unsafe {
        libc::sysctlbyname(
            b"machdep.cpu.brand_string\0".as_ptr() as *const c_char,
            cpuBrandString.as_mut_ptr() as *mut c_void,
            &mut cpuBrandStringSize,
            ptr::null_mut(),
            0,
        )
    };
    if ret == -1 {
        let err = std::io::Error::last_os_error();
        eprintln!(
            "WARN: Unable to determine the CPU brand string.\nerrno: {}, {}",
            err.raw_os_error().unwrap_or(0),
            err
        );

        String_safeStrncpy(cpuBrandString, b"UNKNOWN!");
    }
}

/// Port of `bool Platform_isRunningTranslated(void)`
/// (`darwin/PlatformHelpers.c:72`).
// Adapted from https://developer.apple.com/documentation/apple-silicon/about-the-rosetta-translation-environment
pub fn Platform_isRunningTranslated() -> bool {
    let mut ret: c_int = 0;
    let mut size = size_of::<c_int>();
    let rc = unsafe {
        libc::sysctlbyname(
            b"sysctl.proc_translated\0".as_ptr() as *const c_char,
            &mut ret as *mut c_int as *mut c_void,
            &mut size,
            ptr::null_mut(),
            0,
        )
    };
    if rc == -1 {
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::ENOENT) {
            return false;
        }

        eprintln!(
            "WARN: Could not determine if this process was running in a translation environment like Rosetta 2.\n\
             Assuming that we're not.\n\
             errno: {}, {}",
            err.raw_os_error().unwrap_or(0),
            err
        );

        return false;
    }
    ret != 0
}
