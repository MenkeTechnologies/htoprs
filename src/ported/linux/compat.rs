//! Port of `linux/Compat.c` — thin wrappers around the *at() family of
//! syscalls plus small `read(2)` helpers.
//!
//! This port commits to the modern-Linux build configuration in which
//! `HAVE_FACCESSAT`, `HAVE_FSTATAT`, `HAVE_OPENAT` and `HAVE_READLINKAT`
//! are all defined (see `configure.ac`). Consequently `openat_arg_t` is a
//! file descriptor (`int`) rather than a path, matching the way the Linux
//! process table threads `procFd` handles through the tree walk. The
//! mutually-exclusive fallback branches (`#else`/`#ifndef HAVE_*`) are the
//! other build variant and are intentionally not ported here (see the
//! module port rules on build variants).
#![allow(non_snake_case)]
#![allow(non_camel_case_types)]
#![allow(dead_code)]

use std::ffi::CStr;
use std::io;

use libc::{c_char, c_int, ssize_t};

/// GNU/Hurd does not have `PATH_MAX` in `limits.h`; on Linux it is 4096.
///
/// Port of `Compat.c:24` (`#define PATH_MAX 4096`).
const PATH_MAX: usize = 4096;

/// `typedef int openat_arg_t;` — the `HAVE_OPENAT` variant.
///
/// Port of `Compat.h:31`.
pub type openat_arg_t = c_int;

/// Close the directory handle backing an [`openat_arg_t`] (`HAVE_OPENAT`
/// variant, where the handle is a file descriptor).
///
/// Port of `Compat.h:33`.
pub fn Compat_openatArgClose(dirfd: openat_arg_t) {
    unsafe {
        libc::close(dirfd);
    }
}

/// Port of `Compat.c:28` (`HAVE_FACCESSAT` variant).
pub fn Compat_faccessat(dirfd: c_int, pathname: &CStr, mode: c_int, flags: c_int) -> c_int {
    // C: errno = EINVAL / errno = 0; — set/clear errno portably across
    // glibc/musl (__errno_location) and the BSD/macOS libc (__error).
    let set_errno = |value: c_int| unsafe {
        #[cfg(any(target_os = "linux", target_os = "android"))]
        {
            *libc::__errno_location() = value;
        }
        #[cfg(any(target_os = "netbsd", target_os = "openbsd"))]
        {
            *libc::__errno() = value;
        }
        #[cfg(any(target_os = "solaris", target_os = "illumos"))]
        {
            *libc::___errno() = value;
        }
        #[cfg(not(any(
            target_os = "linux",
            target_os = "android",
            target_os = "netbsd",
            target_os = "openbsd",
            target_os = "solaris",
            target_os = "illumos"
        )))]
        {
            *libc::__error() = value;
        }
    };

    // HAVE_FACCESSAT: try the syscall first; only fall through when it
    // fails with EINVAL (unsupported flag combination on this kernel).
    //
    // Implementation note: AT_SYMLINK_NOFOLLOW unsupported on FreeBSD,
    // fallback to lstat in that case.
    set_errno(0);

    let ret = unsafe { libc::faccessat(dirfd, pathname.as_ptr(), mode, flags) };
    let err = io::Error::last_os_error().raw_os_error().unwrap_or(0);
    if ret == 0 || err != libc::EINVAL {
        return ret;
    }

    // Error out on unsupported configurations.
    if dirfd != libc::AT_FDCWD || mode != libc::F_OK {
        set_errno(libc::EINVAL);
        return -1;
    }

    // Fallback to stat(2)/lstat(2) depending on flags.
    let mut sb: libc::stat = unsafe { std::mem::zeroed() };
    if flags != 0 {
        unsafe { libc::lstat(pathname.as_ptr(), &mut sb) }
    } else {
        unsafe { libc::stat(pathname.as_ptr(), &mut sb) }
    }
}

/// Port of `Compat.c:63` (`HAVE_FSTATAT` variant; `dirpath` unused).
pub fn Compat_fstatat(
    dirfd: c_int,
    _dirpath: &CStr,
    pathname: &CStr,
    statbuf: &mut libc::stat,
    flags: c_int,
) -> c_int {
    unsafe { libc::fstatat(dirfd, pathname.as_ptr(), statbuf as *mut libc::stat, flags) }
}

/// Port of `Compat.h:37` (`HAVE_OPENAT` variant; the `Compat.c:92` fallback
/// that takes a directory path is the other build variant and is not ported).
pub fn Compat_openat(dirfd: openat_arg_t, pathname: &CStr, flags: c_int) -> c_int {
    unsafe { libc::openat(dirfd, pathname.as_ptr(), flags) }
}

/// Port of `Compat.c:104` (`HAVE_READLINKAT` variant; `dirpath` unused).
pub fn Compat_readlinkat(
    dirfd: c_int,
    _dirpath: &CStr,
    pathname: &CStr,
    buf: &mut [u8],
) -> ssize_t {
    unsafe {
        libc::readlinkat(
            dirfd,
            pathname.as_ptr(),
            buf.as_mut_ptr() as *mut c_char,
            buf.len(),
        )
    }
}

/// Port of `Compat.c:128` (`HAVE_OPENAT` variant).
pub fn Compat_readlink(dirfd: openat_arg_t, pathname: &CStr, buf: &mut [u8]) -> ssize_t {
    // C: xSnprintf(fdPath, ..., "/proc/self/fd/%d", dirfd);
    let fd_path = format!("/proc/self/fd/{dirfd}\0");

    // C: char dirPath[PATH_MAX + 1];
    //    r = readlink(fdPath, dirPath, sizeof(dirPath) - 1);
    let mut dir_path = [0u8; PATH_MAX + 1];
    let r = unsafe {
        libc::readlink(
            fd_path.as_ptr() as *const c_char,
            dir_path.as_mut_ptr() as *mut c_char,
            dir_path.len() - 1,
        )
    };
    if r < 0 {
        return r;
    }

    // C: dirPath[r] = '\0';
    dir_path[r as usize] = 0;

    // C: xSnprintf(linkPath, ..., "%s/%s", dirPath, pathname);
    // Build the path as bytes to preserve non-UTF-8 filesystem paths.
    let mut link_path: Vec<u8> = Vec::with_capacity(r as usize + 1 + pathname.to_bytes().len() + 1);
    link_path.extend_from_slice(&dir_path[..r as usize]);
    link_path.push(b'/');
    link_path.extend_from_slice(pathname.to_bytes());
    link_path.push(0);

    unsafe {
        libc::readlink(
            link_path.as_ptr() as *const c_char,
            buf.as_mut_ptr() as *mut c_char,
            buf.len(),
        )
    }
}

/// Port of `Compat.c:159`.
fn readfd_internal(fd: c_int, buf: &mut [u8]) -> ssize_t {
    if buf.is_empty() {
        unsafe {
            libc::close(fd);
        }
        return -(libc::EINVAL as ssize_t);
    }

    let mut already_read: ssize_t = 0;
    let mut count = buf.len() - 1; // reserve one for null-terminator
    let mut offset: usize = 0;

    loop {
        let res = unsafe { libc::read(fd, buf[offset..].as_mut_ptr() as *mut libc::c_void, count) };
        if res == -1 {
            let raw = io::Error::last_os_error().raw_os_error().unwrap_or(0);
            if raw == libc::EINTR {
                continue;
            }

            unsafe {
                libc::close(fd);
            }
            buf[offset] = b'\0';
            return -(raw as ssize_t);
        }

        if res > 0 {
            debug_assert!(res as usize <= count);

            offset += res as usize;
            count -= res as usize;
            already_read += res;
        }

        if count == 0 || res == 0 {
            unsafe {
                libc::close(fd);
            }
            buf[offset] = b'\0';
            return already_read;
        }
    }
}

/// Port of `Compat.c:195`.
pub fn Compat_readfile(pathname: &CStr, buf: &mut [u8]) -> ssize_t {
    let fd = unsafe { libc::open(pathname.as_ptr(), libc::O_RDONLY) };
    if fd < 0 {
        return -(io::Error::last_os_error().raw_os_error().unwrap_or(0) as ssize_t);
    }

    readfd_internal(fd, buf)
}

/// Port of `Compat.c:203`.
pub fn Compat_readfileat(dirfd: openat_arg_t, pathname: &CStr, buf: &mut [u8]) -> ssize_t {
    let fd = Compat_openat(dirfd, pathname, libc::O_RDONLY);
    if fd < 0 {
        return -(io::Error::last_os_error().raw_os_error().unwrap_or(0) as ssize_t);
    }

    readfd_internal(fd, buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;
    use std::io::Write;

    fn scratch_path(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("htoprs_compat_{}_{}", std::process::id(), name));
        p
    }

    #[test]
    fn readfile_reads_contents_and_null_terminates() {
        let path = scratch_path("readfile");
        {
            let mut f = std::fs::File::create(&path).unwrap();
            f.write_all(b"hello").unwrap();
        }
        let c_path = CString::new(path.to_str().unwrap()).unwrap();

        let mut buf = [0u8; 64];
        let n = Compat_readfile(&c_path, &mut buf);
        assert_eq!(n, 5);
        assert_eq!(&buf[..5], b"hello");
        assert_eq!(buf[5], 0);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn readfile_missing_returns_negative_errno() {
        let path = scratch_path("does_not_exist");
        let _ = std::fs::remove_file(&path);
        let c_path = CString::new(path.to_str().unwrap()).unwrap();

        let mut buf = [0u8; 16];
        let n = Compat_readfile(&c_path, &mut buf);
        assert_eq!(n, -(libc::ENOENT as ssize_t));
    }

    #[test]
    fn readfile_zero_length_buffer_is_einval() {
        let path = scratch_path("readfile_empty_buf");
        {
            let mut f = std::fs::File::create(&path).unwrap();
            f.write_all(b"x").unwrap();
        }
        let c_path = CString::new(path.to_str().unwrap()).unwrap();

        let mut buf: [u8; 0] = [];
        let n = Compat_readfile(&c_path, &mut buf);
        assert_eq!(n, -(libc::EINVAL as ssize_t));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn faccessat_existence_check() {
        let path = scratch_path("faccessat");
        {
            let mut f = std::fs::File::create(&path).unwrap();
            f.write_all(b"y").unwrap();
        }
        let c_path = CString::new(path.to_str().unwrap()).unwrap();

        assert_eq!(Compat_faccessat(libc::AT_FDCWD, &c_path, libc::F_OK, 0), 0);

        std::fs::remove_file(&path).unwrap();
        assert_eq!(Compat_faccessat(libc::AT_FDCWD, &c_path, libc::F_OK, 0), -1);
    }
}
