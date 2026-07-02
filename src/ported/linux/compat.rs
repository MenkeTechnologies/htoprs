//! Stub scaffold for `Compat.c` — NOT yet ported.
//!
//! Every `pub fn` below is a placeholder (`todo!()`) named after a real
//! htop C function so the port-purity gate accepts the module and the
//! port surface is laid out. Replace each stub with a faithful port of
//! the C body, updating the signature and the doc comment to `Port of
//! `Compat.c`:<line>.` as you go. `gen_port_report.py` counts these
//! `todo!()` bodies as *stubbed*, not *ported*, so scaffolding does not
//! inflate coverage.
#![allow(non_snake_case)]
#![allow(dead_code)]


/// TODO: port of `int Compat_faccessat(int dirfd,` from `Compat.c:28`.
pub fn Compat_faccessat() {
    todo!("port of Compat.c:28")
}

/// TODO: port of `int Compat_fstatat(int dirfd,` from `Compat.c:63`.
pub fn Compat_fstatat() {
    todo!("port of Compat.c:63")
}

/// TODO: port of `int Compat_openat(const char* dirpath,` from `Compat.c:92`.
pub fn Compat_openat() {
    todo!("port of Compat.c:92")
}

/// TODO: port of `ssize_t Compat_readlinkat(int dirfd,` from `Compat.c:104`.
pub fn Compat_readlinkat() {
    todo!("port of Compat.c:104")
}

/// TODO: port of `ssize_t Compat_readlink(openat_arg_t dirfd,` from `Compat.c:128`.
pub fn Compat_readlink() {
    todo!("port of Compat.c:128")
}

/// TODO: port of `static ssize_t readfd_internal(int fd, void* buffer, size_t count` from `Compat.c:159`.
pub fn readfd_internal() {
    todo!("port of Compat.c:159")
}

/// TODO: port of `ssize_t Compat_readfile(const char* pathname, void* buffer, size_t count` from `Compat.c:195`.
pub fn Compat_readfile() {
    todo!("port of Compat.c:195")
}

/// TODO: port of `ssize_t Compat_readfileat(openat_arg_t dirfd, const char* pathname, void* buffer, size_t count` from `Compat.c:203`.
pub fn Compat_readfileat() {
    todo!("port of Compat.c:203")
}
