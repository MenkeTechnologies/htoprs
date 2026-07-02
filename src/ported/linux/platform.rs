//! Partial port of `linux/Platform.c` — htop's Linux platform hooks.
//!
//! Ported here (self-contained: only libc / Rust std / already-ported
//! `String_*` helpers — no unported substrate):
//! - `Platform_getPressureStall` (`Platform.c:643`)
//! - `Platform_getProcessEnv` (`Platform.c:519`)
//! - `Platform_longOptionsUsage` (`Platform.c:994`, non-`HAVE_LIBCAP` build)
//! - `Platform_done` (`Platform.c:1171`, non-`HAVE_SENSORS` build)
//! - `Platform_init` (`Platform.c:1129`)
//!
//! Everything else is still `todo!()` and blocked on unported substrate —
//! chiefly `LinuxMachine`/`CPUData` (whole struct unmodeled), the `Compat_*`
//! file readers (`Compat.c`, no signatures yet), and the meter/panel/battery
//! types (`ACPresence`, `DiskIOData`, `NetworkIOData`, `FileLocks_*`,
//! `CommandLineStatus`, `State`, `MainPanel`, …) owned by other files.
//! `HAVE_LIBCAP`-only functions
//! (`dropCapabilities`, the `Platform_getLongOption`/`longOptionsUsage`
//! capability branches) are the mutually-exclusive alternative build and are
//! not ported (rule 3).
#![allow(non_snake_case)]
#![allow(dead_code)]

use core::sync::atomic::{AtomicBool, Ordering};

use crate::ported::xutils::{String_eq, String_startsWith};

/// `PROCDIR` — the procfs mount htop was compiled to read (a `config.h`
/// macro, default `"/proc"`). Defined locally so this module's `/proc`
/// reads reproduce the C string-literal concatenations verbatim.
const PROCDIR: &str = "/proc";

/// Port of the global `bool Running_containerized` from `Platform.c:87`.
/// Set by [`Platform_init`] when htop detects it is running inside a
/// container. A mutable process-global C `bool`, modeled as an
/// [`AtomicBool`] per the global-mutable-static idiom (rule 4).
#[allow(non_upper_case_globals)] // faithful port of C global `Running_containerized`
pub static Running_containerized: AtomicBool = AtomicBool::new(false);

/// TODO: port of `static Htop_Reaction Platform_actionSetIOPriority(State* st` from `Platform.c:172`.
pub fn Platform_actionSetIOPriority() {
    todo!("port of Platform.c:172")
}

/// TODO: port of `static bool Platform_changeAutogroupPriority(MainPanel* panel, int delta` from `Platform.c:194`.
pub fn Platform_changeAutogroupPriority() {
    todo!("port of Platform.c:194")
}

/// TODO: port of `static Htop_Reaction Platform_actionHigherAutogroupPriority(State* st` from `Platform.c:206`.
pub fn Platform_actionHigherAutogroupPriority() {
    todo!("port of Platform.c:206")
}

/// TODO: port of `static Htop_Reaction Platform_actionLowerAutogroupPriority(State* st` from `Platform.c:214`.
pub fn Platform_actionLowerAutogroupPriority() {
    todo!("port of Platform.c:214")
}

/// TODO: port of `void Platform_setBindings(Htop_Action* keys` from `Platform.c:222`.
pub fn Platform_setBindings() {
    todo!("port of Platform.c:222")
}

/// TODO: port of `int Platform_getUptime(void` from `Platform.c:283`.
pub fn Platform_getUptime() {
    todo!("port of Platform.c:283")
}

/// TODO: port of `void Platform_getLoadAverage(double* one, double* five, double* fifteen` from `Platform.c:302`.
pub fn Platform_getLoadAverage() {
    todo!("port of Platform.c:302")
}

/// TODO: port of `pid_t Platform_getMaxPid(void` from `Platform.c:325`.
pub fn Platform_getMaxPid() {
    todo!("port of Platform.c:325")
}

/// TODO: port of `double Platform_setCPUValues(Meter* this, unsigned int cpu` from `Platform.c:343`.
pub fn Platform_setCPUValues() {
    todo!("port of Platform.c:343")
}

/// TODO: port of `void Platform_setGPUValues(Meter* this, double* totalUsage, unsigned long long* totalGPUTimeDiff` from `Platform.c:395`.
pub fn Platform_setGPUValues() {
    todo!("port of Platform.c:395")
}

/// TODO: port of `void Platform_setMemoryValues(Meter* this` from `Platform.c:441`.
pub fn Platform_setMemoryValues() {
    todo!("port of Platform.c:441")
}

/// TODO: port of `void Platform_setSwapValues(Meter* this` from `Platform.c:469`.
pub fn Platform_setSwapValues() {
    todo!("port of Platform.c:469")
}

/// TODO: port of `void Platform_setZramValues(Meter* this` from `Platform.c:499`.
pub fn Platform_setZramValues() {
    todo!("port of Platform.c:499")
}

/// TODO: port of `void Platform_setZfsArcValues(Meter* this` from `Platform.c:507`.
pub fn Platform_setZfsArcValues() {
    todo!("port of Platform.c:507")
}

/// TODO: port of `void Platform_setZfsCompressedArcValues(Meter* this` from `Platform.c:513`.
pub fn Platform_setZfsCompressedArcValues() {
    todo!("port of Platform.c:513")
}

/// Port of `char* Platform_getProcessEnv(pid_t pid)` from `Platform.c:519`.
/// Reads `PROCDIR/<pid>/environ` (the process's NUL-separated environment
/// block) whole and returns it with two trailing NUL terminators appended,
/// exactly as the C does (`env[size] = env[size+1] = '\0'`).
///
/// Signature mapping: C `pid_t pid` → [`libc::pid_t`]; the C `char*` result
/// / `NULL` → `Option<String>` (idiom rule 4). The C grows a heap buffer in
/// 4096-byte `fread` chunks; the faithful analog reads the file whole
/// (`std::fs::read`). Any open **or** read error yields `None`, matching the
/// C returning `NULL` on `!fp` and on `ferror`/`bytes < 0`. Non-UTF-8 bytes
/// are replaced (`from_utf8_lossy`); the interior and trailing NULs are
/// valid UTF-8 and preserved for the consumer's NUL-splitting.
pub fn Platform_getProcessEnv(pid: libc::pid_t) -> Option<String> {
    let procname = format!("{}/{}/environ", PROCDIR, pid);
    let mut env = std::fs::read(&procname).ok()?;
    env.push(b'\0');
    env.push(b'\0');
    Some(String::from_utf8_lossy(&env).into_owned())
}

/// TODO: port of `FileLocks_ProcessData* Platform_getProcessLocks(pid_t pid` from `Platform.c:555`.
pub fn Platform_getProcessLocks() {
    todo!("port of Platform.c:555")
}

/// Port of `void Platform_getPressureStall(const char* file, bool some, double* ten, double* sixty, double* threehundred)` from `Platform.c:643`.
/// Reads `PROCDIR/pressure/<file>` and returns the 10/60/300-second pressure
/// averages via the three out-params. When the file cannot be opened all
/// three become `NAN`; otherwise they hold the `some` line's `avg10/60/300`,
/// and when `some == false` the `full` line's values overwrite them —
/// reproducing the C's two sequential `fscanf` calls.
///
/// Signature mapping: C `double*` out-params → `&mut f64`; `const char*
/// file` → `&str`. The C `sscanf`/`fscanf` field extraction is done by
/// scanning whitespace tokens for the `avgN=` prefixes. The C's
/// `assert(total == 3)` becomes a `debug_assert!` on having parsed all three
/// averages of the selected line.
pub fn Platform_getPressureStall(
    file: &str,
    some: bool,
    ten: &mut f64,
    sixty: &mut f64,
    threehundred: &mut f64,
) {
    *ten = 0.0;
    *sixty = 0.0;
    *threehundred = 0.0;

    let procname = format!("{}/pressure/{}", PROCDIR, file);
    let content = match std::fs::read_to_string(&procname) {
        Ok(c) => c,
        Err(_) => {
            *ten = f64::NAN;
            *sixty = f64::NAN;
            *threehundred = f64::NAN;
            return;
        }
    };

    // Extract avg10/avg60/avg300 from a "some ..."/"full ..." line; returns
    // the three values only if all parsed (the C `fscanf` returning 3).
    let parse_line = |line: &str| -> Option<(f64, f64, f64)> {
        let mut a10: Option<f64> = None;
        let mut a60: Option<f64> = None;
        let mut a300: Option<f64> = None;
        for tok in line.split_whitespace() {
            if let Some(v) = tok.strip_prefix("avg10=") {
                a10 = v.parse().ok();
            } else if let Some(v) = tok.strip_prefix("avg60=") {
                a60 = v.parse().ok();
            } else if let Some(v) = tok.strip_prefix("avg300=") {
                a300 = v.parse().ok();
            }
        }
        match (a10, a60, a300) {
            (Some(x), Some(y), Some(z)) => Some((x, y, z)),
            _ => None,
        }
    };

    // First fscanf: the "some" line.
    let mut total = 0;
    if let Some((x, y, z)) = content
        .lines()
        .find(|l| l.starts_with("some"))
        .and_then(parse_line)
    {
        *ten = x;
        *sixty = y;
        *threehundred = z;
        total = 3;
    }

    // Second fscanf: only when caller wants the "full" line, overwriting.
    if !some {
        total = 0;
        if let Some((x, y, z)) = content
            .lines()
            .find(|l| l.starts_with("full"))
            .and_then(parse_line)
        {
            *ten = x;
            *sixty = y;
            *threehundred = z;
            total = 3;
        }
    }

    debug_assert!(total == 3);
}

/// TODO: port of `void Platform_getFileDescriptors(double* used, double* max` from `Platform.c:661`.
pub fn Platform_getFileDescriptors() {
    todo!("port of Platform.c:661")
}

/// TODO: port of `bool Platform_getDiskIO(DiskIOData* data` from `Platform.c:679`.
pub fn Platform_getDiskIO() {
    todo!("port of Platform.c:679")
}

/// TODO: port of `bool Platform_getNetworkIO(NetworkIOData* data` from `Platform.c:722`.
pub fn Platform_getNetworkIO() {
    todo!("port of Platform.c:722")
}

/// TODO: port of `static double Platform_Battery_getProcBatInfo(void` from `Platform.c:764`.
pub fn Platform_Battery_getProcBatInfo() {
    todo!("port of Platform.c:764")
}

/// TODO: port of `static ACPresence procAcpiCheck(void` from `Platform.c:827`.
pub fn procAcpiCheck() {
    todo!("port of Platform.c:827")
}

/// TODO: port of `static void Platform_Battery_getProcData(double* percent, ACPresence* isOnAC` from `Platform.c:836`.
pub fn Platform_Battery_getProcData() {
    todo!("port of Platform.c:836")
}

/// TODO: port of `static void Platform_Battery_getSysData(double* percent, ACPresence* isOnAC` from `Platform.c:845`.
pub fn Platform_Battery_getSysData() {
    todo!("port of Platform.c:845")
}

/// TODO: port of `void Platform_getBattery(double* percent, ACPresence* isOnAC` from `Platform.c:964`.
pub fn Platform_getBattery() {
    todo!("port of Platform.c:964")
}

/// Port of `void Platform_longOptionsUsage(const char* name)` from
/// `Platform.c:994`. On this build `HAVE_LIBCAP` is undefined, so the C body
/// is just `(void) name;` — a no-op. The `HAVE_LIBCAP` branch (which prints
/// the `--drop-capabilities` help text) is the mutually-exclusive
/// alternative build and is not ported (rule 3).
pub fn Platform_longOptionsUsage(_name: &str) {}

/// TODO: port of `CommandLineStatus Platform_getLongOption(int opt, int argc, char** argv` from `Platform.c:1008`.
pub fn Platform_getLongOption() {
    todo!("port of Platform.c:1008")
}

/// TODO: port of `static int dropCapabilities(enum CapMode mode` from `Platform.c:1044`.
pub fn dropCapabilities() {
    todo!("port of Platform.c:1044")
}

/// Port of `bool Platform_init(void)` from `Platform.c:1129`. Verifies
/// procfs is readable, then detects whether htop is running containerized:
/// first by comparing the `PROCDIR/self/ns/pid` namespace link against the
/// host init inode's magic string, then (if inconclusive) by scanning
/// `PROCDIR/1/mounts` for `lxcfs`/`overlay` markers. Sets
/// [`Running_containerized`] and returns whether init succeeded.
///
/// The `HAVE_LIBCAP` prelude (`dropCapabilities`) and the
/// `HAVE_SENSORS_SENSORS_H` `LibSensors_init()` call are `#if`-omitted on
/// this build, so — like the C preprocessor here — they are simply absent.
/// `access`/`readlink` are called via `libc` (the affinity-module
/// precedent for leaf syscalls); the mounts file is read with `std::fs`
/// (the C `fopen` returning `NULL` maps to the `Err` arm: skip the scan).
pub fn Platform_init() -> bool {
    let procdir = std::ffi::CString::new(PROCDIR).unwrap();
    if unsafe { libc::access(procdir.as_ptr(), libc::R_OK) } != 0 {
        eprintln!(
            "Error: could not read procfs (compiled to look in {}).",
            PROCDIR
        );
        return false;
    }

    let nspath = std::ffi::CString::new(format!("{}/self/ns/pid", PROCDIR)).unwrap();
    let mut target = [0u8; 4096];
    let ret = unsafe {
        libc::readlink(
            nspath.as_ptr(),
            target.as_mut_ptr() as *mut libc::c_char,
            target.len() - 1,
        )
    };
    if ret > 0 {
        // C: target[ret] = '\0'; — slice to the read length instead.
        let link = String::from_utf8_lossy(&target[..ret as usize]);
        // magic constant PROC_PID_INIT_INO from include/linux/proc_ns.h#L46
        if !String_eq("pid:[4026531836]", &link) {
            Running_containerized.store(true, Ordering::Relaxed);
            return true; // early return
        }
    }

    if let Ok(mounts) = std::fs::read_to_string(format!("{}/1/mounts", PROCDIR)) {
        for lineBuffer in mounts.lines() {
            // detect lxc or overlayfs and guess that this means we are running containerized
            if String_startsWith(lineBuffer, "lxcfs /proc")
                || String_startsWith(lineBuffer, "overlay / overlay")
            {
                Running_containerized.store(true, Ordering::Relaxed);
                break;
            }
        }
    }

    true
}

/// Port of `void Platform_done(void)` from `Platform.c:1171`. On this build
/// `HAVE_SENSORS_SENSORS_H` is undefined, so the sole statement
/// (`LibSensors_cleanup()`) is `#if`-omitted and the body is empty. This is
/// not a `free()`/`Drop` teardown — there is nothing to release — so the
/// faithful port of the non-sensors build is a genuine no-op.
pub fn Platform_done() {}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Platform_getProcessEnv` returns `None` (C `NULL`) when the target
    /// `PROCDIR/<pid>/environ` cannot be opened — here an impossible pid, so
    /// the result is deterministic on any host.
    #[test]
    fn getprocessenv_missing_pid_is_none() {
        assert!(Platform_getProcessEnv(2147483646).is_none());
    }

    /// On Linux the current process always has a readable `environ`, so the
    /// result is `Some` and ends with the two NUL terminators the C appends.
    #[cfg(target_os = "linux")]
    #[test]
    fn getprocessenv_self_has_double_nul_terminator() {
        let env = Platform_getProcessEnv(std::process::id() as libc::pid_t)
            .expect("self environ must be readable on Linux");
        assert!(env.ends_with("\0\0"));
    }

    /// `Platform_getPressureStall` sets all three averages to `NAN` when the
    /// pressure file is absent — a nonexistent name reproduces the C
    /// `!fp` branch on any host.
    #[test]
    fn getpressurestall_missing_file_is_nan() {
        let (mut ten, mut sixty, mut threehundred) = (0.0, 0.0, 0.0);
        Platform_getPressureStall(
            "zzz_nonexistent_pressure_file_zzz",
            true,
            &mut ten,
            &mut sixty,
            &mut threehundred,
        );
        assert!(ten.is_nan() && sixty.is_nan() && threehundred.is_nan());
    }

    /// The no-op ports must not panic when invoked.
    #[test]
    fn noop_ports_do_not_panic() {
        Platform_longOptionsUsage("htop");
        Platform_done();
    }
}
