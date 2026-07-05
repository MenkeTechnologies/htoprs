//! Port of `dragonflybsd/DragonFlyBSDMachine.c` + `.h` — the DragonFly BSD
//! per-host state and its `sysctl`/`libkvm` scan layer.
//!
//! The struct model, the small pure accessors, and the pure-`sysctl` scans are
//! ported here. Compiled only under `#[cfg(target_os = "dragonfly")]` and, like
//! the other BSD layers, verified by primary-source reading + the port-purity
//! gate (not a cross-compile).
//!
//! Ported (kvm-free): [`DragonFlyBSDMachine_scanJails`] (`jail.list`
//! sysctlbyname → the `jails` hashtable) and [`DragonFlyBSDMachine_readJailName`]
//! (jailid → hostname lookup).
//!
//! Still stubbed — need `libkvm`, which `libc` does not expose for the DragonFly
//! target: `Machine_new`/`Machine_delete` (`kvm_openfiles`), `Machine_scan`,
//! `DragonFlyBSDMachine_scanCPUTime` (transitively — its `MIB_kern_cp_time*` are
//! set in the kvm-gated `Machine_new`), and `scanMemoryInfo` (`kvm_getswapinfo`).
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use core::ffi::c_void;
use core::ptr;

use crate::ported::crt::CRT_fatalError;
use crate::ported::hashtable::{Hashtable, Hashtable_get, Hashtable_new, Hashtable_put};
use crate::ported::machine::Machine;
use crate::ported::object::{Object, ObjectClass};

/// The `char*` hostname value stored in the `jails` [`Hashtable`] (jailid →
/// hostname). The C stores a raw `xStrdup`'d `char*`; the ported `Hashtable`
/// stores `Object`s, so the string is wrapped (no C struct — the value type is
/// a bare `char*` in htop).
struct JailName(String);

/// Class identity for [`JailName`] (`extends: None`, as the file-local
/// `LibraryData` accumulator elsewhere).
static JailName_class: ObjectClass = ObjectClass { extends: None };

impl Object for JailName {
    fn klass(&self) -> &'static ObjectClass {
        &JailName_class
    }
}

/// Port of `typedef struct CPUData_` (`DragonFlyBSDMachine.h:26`) — the
/// per-CPU load percentages computed each scan from the `kern.cp_time(s)`
/// sysctl deltas.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CPUData {
    pub userPercent: f64,
    pub nicePercent: f64,
    pub systemPercent: f64,
    pub irqPercent: f64,
    pub idlePercent: f64,
    pub systemAllPercent: f64,
}

/// Port of `typedef struct DragonFlyBSDMachine_` (`DragonFlyBSDMachine.h`).
/// "Extends" [`Machine`] via the embedded `super_`, plus the DragonFly kvm
/// handle, jail table, page-size / scale constants, memory partition sizes,
/// per-CPU data, and the `cp_time(s)` old/new tick buffers.
///
/// `kd` (C `kvm_t*`) is an opaque `*mut c_void` (no `libkvm` on non-DragonFly
/// hosts); `jails` (C `Hashtable*` of jailid → hostname) is an owned
/// [`Hashtable`]; `memory_t` fields are `u64`; the `cp_time*` arrays are
/// `Vec<u64>` (C `unsigned long*`, `xCalloc`-sized per CPU).
///
/// No `#[derive(Debug)]`: the `jails` [`Hashtable`] holds trait-object values
/// and is not `Debug`. Constructed by the (stubbed) [`Machine_new`].
pub struct DragonFlyBSDMachine {
    /// C `Machine super`.
    pub super_: Machine,
    /// C `kvm_t* kd` — the libkvm handle (opaque here).
    pub kd: *mut c_void,
    /// C `Hashtable* jails` — jailid → hostname.
    pub jails: Option<Hashtable>,
    /// C `int pageSize`.
    pub pageSize: i32,
    /// C `int pageSizeKb`.
    pub pageSizeKb: i32,
    /// C `int kernelFScale` — kernel fixed-point load scale.
    pub kernelFScale: i32,
    /// C `memory_t wiredMem`.
    pub wiredMem: u64,
    /// C `memory_t buffersMem`.
    pub buffersMem: u64,
    /// C `memory_t activeMem`.
    pub activeMem: u64,
    /// C `memory_t inactiveMem`.
    pub inactiveMem: u64,
    /// C `memory_t cacheMem`.
    pub cacheMem: u64,
    /// C `CPUData* cpus` — one entry per CPU (index 0 is the aggregate).
    pub cpus: Vec<CPUData>,
    /// C `unsigned long* cp_time_o` — previous aggregate cp_time ticks.
    pub cp_time_o: Vec<u64>,
    /// C `unsigned long* cp_time_n` — current aggregate cp_time ticks.
    pub cp_time_n: Vec<u64>,
    /// C `unsigned long* cp_times_o` — previous per-CPU cp_times ticks.
    pub cp_times_o: Vec<u64>,
    /// C `unsigned long* cp_times_n` — current per-CPU cp_times ticks.
    pub cp_times_n: Vec<u64>,
}

/// Port of `bool Machine_isCPUonline(const Machine* host, unsigned int id)`
/// (`DragonFlyBSDMachine.c:369`). DragonFly does not yet expose per-CPU
/// online/offline state, so every existing CPU is reported online (verbatim
/// C behavior, including the `id < existingCPUs` precondition).
pub fn Machine_isCPUonline(host: &Machine, id: u32) -> bool {
    debug_assert!(id < host.existingCPUs);
    // TODO (as in C): support detecting online / offline CPUs.
    true
}

/// Port of `int Machine_getCPUPhysicalCoreID(const Machine* host, unsigned int
/// id)` (`DragonFlyBSDMachine.c:377`). DragonFly does not expose topology, so
/// the physical core id is the CPU id itself.
pub fn Machine_getCPUPhysicalCoreID(host: &Machine, id: u32) -> i32 {
    debug_assert!(id < host.existingCPUs);
    id as i32
}

/// Port of `int Machine_getCPUThreadIndex(const Machine* host, unsigned int
/// id)` (`DragonFlyBSDMachine.c:383`). No SMT topology on DragonFly, so every
/// CPU is thread index 0.
pub fn Machine_getCPUThreadIndex(host: &Machine, id: u32) -> i32 {
    debug_assert!(id < host.existingCPUs);
    0
}

/// TODO: port of `Machine* Machine_new(UsersTable* usersTable, uid_t userId)`
/// (`DragonFlyBSDMachine.c:41`). Opens `kvm_openfiles`, reads page size /
/// physmem / v_page_count via `sysctl(name)tomib`, and allocates the CPU /
/// cp_time buffers — DragonFly `sys/sysctl.h` + `libkvm`, gated to
/// `#[cfg(target_os = "dragonfly")]` when ported.
pub fn Machine_new() {
    todo!("port of DragonFlyBSDMachine.c:41 — kvm_openfiles + sysctl (DragonFly-only)")
}

/// TODO: port of `void Machine_delete(Machine* super)`
/// (`DragonFlyBSDMachine.c:119`). `kvm_close` + `Hashtable_delete(jails)` +
/// `free` of the cpu / cp_time buffers; Rust `Drop` releases the owned Vecs,
/// but the `kvm_t*` close is DragonFly-only.
pub fn Machine_delete() {
    todo!("port of DragonFlyBSDMachine.c:119 — kvm_close teardown (DragonFly-only)")
}

/// TODO: port of `static void DragonFlyBSDMachine_scanCPUTime(Machine* super)`
/// (`DragonFlyBSDMachine.c:141`). Reads `kern.cp_time` / `kern.cp_times` via
/// sysctl and computes per-CPU load deltas. DragonFly sysctl.
pub fn DragonFlyBSDMachine_scanCPUTime() {
    todo!("port of DragonFlyBSDMachine.c:141 — kern.cp_time sysctl (DragonFly-only)")
}

/// TODO: port of `static void DragonFlyBSDMachine_scanMemoryInfo(Machine*
/// super)` (`DragonFlyBSDMachine.c:223`). Reads the `vm.stats.vm.*` counters
/// via sysctl for wired/active/inactive/cache/buffers memory. DragonFly sysctl.
pub fn DragonFlyBSDMachine_scanMemoryInfo() {
    todo!("port of DragonFlyBSDMachine.c:223 — vm.stats sysctl (DragonFly-only)")
}

/// Port of `static void DragonFlyBSDMachine_scanJails(DragonFlyBSDMachine*
/// this)` (`DragonFlyBSDMachine.c:294`). Rebuilds the `jails` hashtable
/// (jailid → hostname) from the `jail.list` sysctlbyname, retrying on `ENOMEM`
/// (the list can grow between sizing and reading). Kvm-free.
pub fn DragonFlyBSDMachine_scanJails(this: &mut DragonFlyBSDMachine) {
    // sysctlbyname("jail.list", NULL, &len, NULL, 0) — get the buffer length.
    let name = c"jail.list";
    let mut len: usize = 0;
    if unsafe { libc::sysctlbyname(name.as_ptr(), ptr::null_mut(), &mut len, ptr::null_mut(), 0) }
        == -1
    {
        CRT_fatalError("initial sysctlbyname / jail.list failed");
    }

    // retry: on ENOMEM the list grew between the sizing and the read.
    loop {
        if len == 0 {
            return;
        }

        let mut jails = vec![0u8; len];
        let rc = unsafe {
            libc::sysctlbyname(
                name.as_ptr(),
                jails.as_mut_ptr() as *mut c_void,
                &mut len,
                ptr::null_mut(),
                0,
            )
        };
        if rc == -1 {
            if std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOMEM) {
                continue; // goto retry
            }
            CRT_fatalError("sysctlbyname / jail.list failed");
        }

        // if (this->jails) Hashtable_delete(this->jails); this->jails = Hashtable_new(20, true);
        this.jails = Some(Hashtable_new(20, true));
        let ht = this.jails.as_mut().unwrap();

        // Walk newline-separated "jailid hostname ..." records; the first two
        // space-delimited tokens are the id and hostname (C strtok on " ").
        let text = String::from_utf8_lossy(&jails[..len.min(jails.len())]);
        for line in text.split('\n') {
            if line.is_empty() {
                continue;
            }
            let mut tok = line.split(' ').filter(|s| !s.is_empty());
            let jailid: i32 = match tok.next() {
                Some(w) => w.parse().unwrap_or(0),
                None => continue,
            };
            let hostname = tok.next().unwrap_or("");
            // if (Hashtable_get(jails, jailid) == NULL) put xStrdup(hostname).
            if Hashtable_get(ht, jailid as u32).is_none() {
                Hashtable_put(ht, jailid as u32, Box::new(JailName(hostname.to_string())));
            }
        }

        return;
    }
}

/// Port of `char* DragonFlyBSDMachine_readJailName(const DragonFlyBSDMachine*
/// host, int jailid)` (`DragonFlyBSDMachine.c:348`). Looks up `jailid` in the
/// [`DragonFlyBSDMachine_scanJails`]-populated `jails` hashtable and returns a
/// copy of the hostname ([`JailName`]), or `"-"` when absent. The C `char*`
/// return becomes an owned `String`.
pub fn DragonFlyBSDMachine_readJailName(host: &DragonFlyBSDMachine, jailid: i32) -> String {
    // if (jailid != 0 && host->jails && (hostname = Hashtable_get(jails, jailid)))
    //    jname = xStrdup(hostname); else jname = xStrdup("-");
    if jailid != 0 {
        if let Some(ht) = &host.jails {
            if let Some(obj) = Hashtable_get(ht, jailid as u32) {
                if let Some(jn) = (obj as &dyn core::any::Any).downcast_ref::<JailName>() {
                    return jn.0.clone();
                }
            }
        }
    }
    "-".to_string()
}

/// TODO: port of `void Machine_scan(Machine* super)`
/// (`DragonFlyBSDMachine.c:361`). Orchestrates the per-tick scan:
/// `DragonFlyBSDMachine_scanMemoryInfo` + `DragonFlyBSDMachine_scanCPUTime`
/// (both stubbed above, DragonFly sysctl).
pub fn Machine_scan() {
    todo!("port of DragonFlyBSDMachine.c:361 — drives the stubbed sysctl scans (DragonFly-only)")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pure CPU accessors report DragonFly's fixed topology answers for
    /// every existing CPU.
    #[test]
    fn cpu_accessors_report_fixed_topology() {
        let mut host = Machine::default();
        host.existingCPUs = 4;
        for id in 0..host.existingCPUs {
            assert!(Machine_isCPUonline(&host, id));
            assert_eq!(Machine_getCPUPhysicalCoreID(&host, id), id as i32);
            assert_eq!(Machine_getCPUThreadIndex(&host, id), 0);
        }
    }
}
