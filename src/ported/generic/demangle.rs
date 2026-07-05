//! Port of `generic/Demangle.c` — C++/Rust symbol demangling for the backtrace
//! screen, htop's `HAVE_DEMANGLING` build variant.
//!
//! Behind the `demangle` cargo feature (off by default), the tier-3 model: the
//! `cplus_demangle` surface is hand-declared and only links `libiberty` when the
//! feature is enabled on a host that has it (htop's `HAVE_LIBIBERTY_CPLUS_DEMANGLE`
//! path). Verified by primary-source reading + the port-purity gate.
//!
//! Ports the `HAVE_LIBIBERTY_CPLUS_DEMANGLE` branch (the htop default). The
//! alternate `HAVE_LIBDEMANGLE_CPLUS_DEMANGLE` branch (a differently-shaped
//! `cplus_demangle(mangled, buf, size)` from the flawed libdemangle API) is not
//! modeled — htop picks one at configure time; the port picks the libiberty one.
#![allow(non_snake_case)]
#![allow(dead_code)]

use std::ffi::CStr;
use std::os::raw::{c_char, c_int};

// Options passed to `cplus_demangle` (2nd parameter), transcribed from
// libiberty's `include/demangle.h`.
const DMGL_PARAMS: c_int = 1 << 0; // include function args
const DMGL_VERBOSE: c_int = 1 << 3; // implementation details (Rust crate disambiguators)
const DMGL_TYPES: c_int = 1 << 4; // also demangle type encodings
const DMGL_AUTO: c_int = 1 << 8; // auto-detect the mangling scheme

#[link(name = "iberty")]
extern "C" {
    /// `char* cplus_demangle(const char* mangled, int options)` (libiberty
    /// `demangle.h`) — returns a `malloc`'d demangled name, or NULL.
    fn cplus_demangle(mangled: *const c_char, options: c_int) -> *mut c_char;
}

/// Port of `char* Demangle_demangle(const char* mangled)`
/// (`generic/Demangle.c:21`, `HAVE_LIBIBERTY_CPLUS_DEMANGLE` branch). Requests as
/// many details as possible (`DMGL_AUTO | DMGL_TYPES | DMGL_VERBOSE |
/// DMGL_PARAMS`, htop's default). Returns the demangled name as an owned
/// `String` (`None` = C `NULL`); the C hands the caller the `malloc`'d `char*`
/// to `free`, so the port copies it out and frees the libiberty buffer.
pub fn Demangle_demangle(mangled: &CStr) -> Option<String> {
    // int options = DMGL_AUTO | DMGL_TYPES | DMGL_VERBOSE | DMGL_PARAMS;
    let options = DMGL_AUTO | DMGL_TYPES | DMGL_VERBOSE | DMGL_PARAMS;

    // return cplus_demangle(mangled, options);
    let out = unsafe { cplus_demangle(mangled.as_ptr(), options) };
    if out.is_null() {
        return None;
    }
    // Own the result and release the libiberty `malloc`'d buffer (the C caller's
    // `free`).
    let demangled = unsafe { CStr::from_ptr(out) }
        .to_string_lossy()
        .into_owned();
    unsafe { libc::free(out as *mut libc::c_void) };
    Some(demangled)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The demangling option mask matches htop's default
    /// (`DMGL_AUTO|DMGL_TYPES|DMGL_VERBOSE|DMGL_PARAMS` = 0x119).
    #[test]
    fn option_mask_matches_htop_default() {
        assert_eq!(DMGL_AUTO | DMGL_TYPES | DMGL_VERBOSE | DMGL_PARAMS, 0x119);
    }
}
