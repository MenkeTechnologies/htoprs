//! Faithful port of htop `generic/fdstat_sysctl.c` — the `sysctl`-based
//! open/max file-descriptor query shared by the darwin and *BSD platforms.

#[cfg(any(
    target_os = "macos",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd"
))]
use libc::c_void;

/// Port of `void Generic_getFileDescriptors_sysctl(double* used, double* max)`
/// (`generic/fdstat_sysctl.c:64`). Fills `used`/`max` with the open and maximum
/// file-descriptor counts using per-platform `sysctlbyname` names. Only defined
/// for the platforms htop's `#if`/`#elif` chain covers (darwin/dragonfly/
/// freebsd/netbsd); the C `#else` is a compile `#error`, so other platforms use
/// their own `Platform_getFileDescriptors` instead of this helper.
#[cfg(any(
    target_os = "macos",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd"
))]
pub fn Generic_getFileDescriptors_sysctl(used: &mut f64, max: &mut f64) {
    // Port of the file-static `Generic_getFileDescriptors_sysctl_internal`
    // (`fdstat_sysctl.c:20`) — nested so the module's only depth-0 symbol is
    // the public C function. `sysctlname_*` are NUL-terminated byte strings
    // (`None` == the C `NULL`).
    fn internal(
        sysctlname_maxfiles: Option<&[u8]>,
        sysctlname_numfiles: Option<&[u8]>,
        size_header: usize,
        size_entry: usize,
        used: &mut f64,
        max: &mut f64,
    ) {
        *used = f64::NAN;
        *max = 65536.0;

        let mut len: libc::size_t;

        let mut max_fd: libc::c_int = 0;
        len = size_of::<libc::c_int>();
        if let Some(name) = sysctlname_maxfiles {
            let rc = unsafe {
                libc::sysctlbyname(
                    name.as_ptr() as *const libc::c_char,
                    &mut max_fd as *mut libc::c_int as *mut c_void,
                    &mut len,
                    std::ptr::null_mut(),
                    0,
                )
            };
            if rc == 0 {
                *max = if max_fd != 0 { max_fd as f64 } else { f64::NAN };
            }
        }

        let mut open_fd: libc::c_int = 0;
        len = size_of::<libc::c_int>();
        if let Some(name) = sysctlname_numfiles {
            let rc = unsafe {
                libc::sysctlbyname(
                    name.as_ptr() as *const libc::c_char,
                    &mut open_fd as *mut libc::c_int as *mut c_void,
                    &mut len,
                    std::ptr::null_mut(),
                    0,
                )
            };
            if rc == 0 {
                *used = open_fd as f64;
                return;
            }
        }

        // If no sysctl arc available, try to guess from the file table size at
        // kern.file. The size per entry differs per OS, thus skip if unknown.
        if size_entry == 0 {
            return;
        }

        len = 0;
        let rc = unsafe {
            libc::sysctlbyname(
                b"kern.file\0".as_ptr() as *const libc::c_char,
                std::ptr::null_mut(),
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        if rc < 0 {
            return;
        }
        if len < size_header {
            return;
        }

        *used = ((len - size_header) / size_entry) as f64;
    }

    #[cfg(target_os = "macos")]
    internal(
        Some(b"kern.maxfiles\0"),
        Some(b"kern.num_files\0"),
        0,
        0,
        used,
        max,
    );
    #[cfg(target_os = "dragonfly")]
    internal(
        Some(b"kern.maxfiles\0"),
        Some(b"kern.openfiles\0"),
        0,
        0,
        used,
        max,
    );
    #[cfg(target_os = "freebsd")]
    internal(
        Some(b"kern.maxfiles\0"),
        Some(b"kern.openfiles\0"),
        0,
        0,
        used,
        max,
    );
    #[cfg(target_os = "netbsd")]
    internal(
        Some(b"kern.maxfiles\0"),
        None,
        0,
        size_of::<libc::kinfo_file>(),
        used,
        max,
    );
}
