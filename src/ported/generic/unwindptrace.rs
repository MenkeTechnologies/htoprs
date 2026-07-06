//! Port of `generic/UnwindPtrace.c` — the libunwind-ptrace backtrace backend
//! for htop's backtrace screen (its `HAVE_LIBUNWIND_PTRACE` build variant).
//!
//! Behind the `unwind` cargo feature (off by default), the same tier-3 model as
//! the sibling `demangle` port: the libunwind surface is hand-declared in
//! `extern` blocks and only links libunwind when the feature is enabled on a
//! host that has it (htop's `HAVE_LIBUNWIND_PTRACE` path). Verified by
//! primary-source reading of the libunwind headers + the port-purity gate;
//! libunwind does not exist on macOS, so `cargo check --features unwind`
//! type-checks the FFI without linking.
//!
//! ## libunwind symbol mangling
//! The generic `unw_*` names in `<libunwind.h>` are macros that paste the
//! remote-unwind prefix `_U<UNW_TARGET>_` onto the bare function name
//! (`libunwind-common.h.in:48,54`: `UNW_OBJ(fn) = _U<arch>_<fn>`). So the real
//! link symbols are `_Ux86_64_step` / `_Uaarch64_step` etc. — declared here via
//! per-arch `#[link_name]`. The `_UPT_*` ptrace-helper symbols
//! (`libunwind-ptrace.h`) are NOT arch-prefixed; they keep their literal names.
//!
//! ## Cursor length / IP register (arch-dependent, from the libunwind headers)
//! `UNW_TDEP_CURSOR_LEN` = 127 on x86_64 (`libunwind-x86_64.h:54`), 250 on
//! aarch64 (`libunwind-aarch64.h:60`). `UNW_REG_IP = UNW_TDEP_IP`
//! (`libunwind-common.h.in:84`): `UNW_X86_64_RIP` = 16 on x86_64,
//! `UNW_AARCH64_X30` = 30 on aarch64. Only x86_64/aarch64 are modeled; any other
//! arch trips a `compile_error!`.
#![allow(non_snake_case)]
#![allow(non_camel_case_types)]
#![allow(dead_code)]

use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_void};

use libc::pid_t;

use crate::ported::backtracescreen::{
    BacktraceFrameData, BacktraceFrameData_delete, BacktraceFrameData_new,
};

// The `unwind` feature only models the two architectures whose cursor length and
// IP register number are transcribed below. `compile_error!` on anything else
// rather than silently linking with a wrong-sized cursor.
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
compile_error!(
    "the `unwind` feature supports only x86_64 and aarch64 (UNW_TDEP_CURSOR_LEN / UNW_REG_IP are hand-transcribed for those two arches)"
);

// `typedef uint64_t unw_word_t;` (`libunwind-x86_64.h:56`).
type unw_word_t = u64;

// `typedef struct unw_addr_space* unw_addr_space_t;` (`libunwind-common.h.in:140`)
// — an opaque pointer we only ever pass through.
type unw_addr_space_t = *mut c_void;

// `#define UNW_TDEP_CURSOR_LEN` — the `unw_word_t[]` blob backing `unw_cursor_t`.
#[cfg(target_arch = "x86_64")]
const CURSOR_LEN: usize = 127;
#[cfg(target_arch = "aarch64")]
const CURSOR_LEN: usize = 250;

// `UNW_REG_IP = UNW_TDEP_IP` — the instruction-pointer register index passed to
// `unw_get_reg`. `UNW_X86_64_RIP` = 16 / `UNW_AARCH64_X30` = 30 (enum position).
#[cfg(target_arch = "x86_64")]
const UNW_REG_IP: c_int = 16;
#[cfg(target_arch = "aarch64")]
const UNW_REG_IP: c_int = 30;

/// `typedef struct { unw_word_t opaque[UNW_TDEP_CURSOR_LEN]; } unw_cursor_t;`
/// (`libunwind-common.h.in:114`). Its fields are never read by the port — it is
/// only ever passed by pointer to the `unw_*` calls, which fill and step it.
#[repr(C)]
struct unw_cursor_t {
    opaque: [unw_word_t; CURSOR_LEN],
}

/// `typedef struct unw_accessors { … } unw_accessors_t;`
/// (`libunwind-common.h.in:175`) — the 11 remote-access callback pointers. The
/// port never reads them: it only takes the address of the `_UPT_accessors`
/// global to hand to `unw_create_addr_space`, so the struct is modeled as an
/// opaque pointer-sized-slot array (the layout is irrelevant to `&`).
#[repr(C)]
struct unw_accessors_t {
    _callbacks: [*mut c_void; 11],
}

// libunwind splits into three libs: `libunwind` (generic dispatch), the arch
// library `libunwind-generic` (the `_U<arch>_*` remote-unwind symbols), and
// `libunwind-ptrace` (the `_UPT_*` helpers). htop links them via the
// `libunwind-ptrace` pkg-config closure; the same closure is declared here.
#[link(name = "unwind")]
#[link(name = "unwind-generic")]
#[link(name = "unwind-ptrace")]
extern "C" {
    // `unw_addr_space_t unw_create_addr_space(unw_accessors_t*, int)`.
    #[cfg_attr(target_arch = "x86_64", link_name = "_Ux86_64_create_addr_space")]
    #[cfg_attr(target_arch = "aarch64", link_name = "_Uaarch64_create_addr_space")]
    fn unw_create_addr_space(accessors: *mut unw_accessors_t, byteorder: c_int)
        -> unw_addr_space_t;

    // `void unw_destroy_addr_space(unw_addr_space_t)`.
    #[cfg_attr(target_arch = "x86_64", link_name = "_Ux86_64_destroy_addr_space")]
    #[cfg_attr(target_arch = "aarch64", link_name = "_Uaarch64_destroy_addr_space")]
    fn unw_destroy_addr_space(addr_space: unw_addr_space_t);

    // `int unw_init_remote(unw_cursor_t*, unw_addr_space_t, void*)`.
    #[cfg_attr(target_arch = "x86_64", link_name = "_Ux86_64_init_remote")]
    #[cfg_attr(target_arch = "aarch64", link_name = "_Uaarch64_init_remote")]
    fn unw_init_remote(
        cursor: *mut unw_cursor_t,
        addr_space: unw_addr_space_t,
        ptr: *mut c_void,
    ) -> c_int;

    // `int unw_step(unw_cursor_t*)`.
    #[cfg_attr(target_arch = "x86_64", link_name = "_Ux86_64_step")]
    #[cfg_attr(target_arch = "aarch64", link_name = "_Uaarch64_step")]
    fn unw_step(cursor: *mut unw_cursor_t) -> c_int;

    // `int unw_get_reg(unw_cursor_t*, int, unw_word_t*)`.
    #[cfg_attr(target_arch = "x86_64", link_name = "_Ux86_64_get_reg")]
    #[cfg_attr(target_arch = "aarch64", link_name = "_Uaarch64_get_reg")]
    fn unw_get_reg(cursor: *mut unw_cursor_t, reg: c_int, val: *mut unw_word_t) -> c_int;

    // `int unw_is_signal_frame(unw_cursor_t*)`.
    #[cfg_attr(target_arch = "x86_64", link_name = "_Ux86_64_is_signal_frame")]
    #[cfg_attr(target_arch = "aarch64", link_name = "_Uaarch64_is_signal_frame")]
    fn unw_is_signal_frame(cursor: *mut unw_cursor_t) -> c_int;

    // `int unw_get_proc_name(unw_cursor_t*, char*, size_t, unw_word_t*)`.
    #[cfg_attr(target_arch = "x86_64", link_name = "_Ux86_64_get_proc_name")]
    #[cfg_attr(target_arch = "aarch64", link_name = "_Uaarch64_get_proc_name")]
    fn unw_get_proc_name(
        cursor: *mut unw_cursor_t,
        buf: *mut c_char,
        len: usize,
        offset: *mut unw_word_t,
    ) -> c_int;

    // `int unw_get_elf_filename(unw_cursor_t*, char*, size_t, unw_word_t*)`.
    #[cfg_attr(target_arch = "x86_64", link_name = "_Ux86_64_get_elf_filename")]
    #[cfg_attr(target_arch = "aarch64", link_name = "_Uaarch64_get_elf_filename")]
    fn unw_get_elf_filename(
        cursor: *mut unw_cursor_t,
        buf: *mut c_char,
        len: usize,
        offset: *mut unw_word_t,
    ) -> c_int;

    // `void* _UPT_create(pid_t)` / `void _UPT_destroy(void*)` — not arch-prefixed
    // (`libunwind-ptrace.h:41,42`). C's `struct UPT_info*` is opaque → `*mut c_void`.
    fn _UPT_create(pid: pid_t) -> *mut c_void;
    fn _UPT_destroy(context: *mut c_void);

    // `extern unw_accessors_t _UPT_accessors;` (`libunwind-ptrace.h:60`) — the
    // ready-made ptrace accessor table; the port only takes its address.
    static _UPT_accessors: unw_accessors_t;
}

/// `strerror(e)` as an owned string, for the `xAsprintf` error messages.
fn strerror(e: c_int) -> String {
    std::io::Error::from_raw_os_error(e).to_string()
}

/// Port of `static int ptraceAttach(pid_t pid)` (`UnwindPtrace.c:32`). Returns 0
/// on success or `errno` on failure, per the C `!ptrace(...) ? 0 : errno`.
#[cfg(target_os = "linux")]
fn ptraceAttach(pid: pid_t) -> c_int {
    let r = unsafe {
        libc::ptrace(
            libc::PTRACE_ATTACH,
            pid,
            std::ptr::null_mut::<c_void>(),
            std::ptr::null_mut::<c_void>(),
        )
    };
    if r == 0 {
        0
    } else {
        std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
    }
}

#[cfg(any(
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "macos"
))]
fn ptraceAttach(pid: pid_t) -> c_int {
    let r = unsafe { libc::ptrace(libc::PT_ATTACH, pid, std::ptr::null_mut::<c_char>(), 0) };
    if r == 0 {
        0
    } else {
        std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
    }
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "macos"
)))]
fn ptraceAttach(_pid: pid_t) -> c_int {
    libc::ENOSYS
}

/// Port of `static int ptraceDetach(pid_t pid)` (`UnwindPtrace.c:43`). Mirror of
/// [`ptraceAttach`] with `PTRACE_DETACH` / `PT_DETACH`.
#[cfg(target_os = "linux")]
fn ptraceDetach(pid: pid_t) -> c_int {
    let r = unsafe {
        libc::ptrace(
            libc::PTRACE_DETACH,
            pid,
            std::ptr::null_mut::<c_void>(),
            std::ptr::null_mut::<c_void>(),
        )
    };
    if r == 0 {
        0
    } else {
        std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
    }
}

#[cfg(any(
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "macos"
))]
fn ptraceDetach(pid: pid_t) -> c_int {
    let r = unsafe { libc::ptrace(libc::PT_DETACH, pid, std::ptr::null_mut::<c_char>(), 0) };
    if r == 0 {
        0
    } else {
        std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
    }
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "macos"
)))]
fn ptraceDetach(_pid: pid_t) -> c_int {
    libc::ENOSYS
}

/// Size of the C `char buffer[2048]` reused for the ELF filename / proc name.
const BUFFER_LEN: usize = 2048;

/// Port of `void UnwindPtrace_makeBacktrace(Vector* frames, pid_t pid, char** error)`
/// (`generic/UnwindPtrace.c:54`).
///
/// Signature matches the ported consumer (`backtracescreen::BacktracePanel_makeBacktrace`):
/// the C `Vector* frames` of `BacktraceFrameData*` is the owned
/// `Vec<BacktraceFrameData>` the panel stores, `Vector_add` becomes
/// `Vec::push`, and the C `char** error` out-param becomes `error: &mut
/// Option<String>` (`None` = C `NULL`). `xStrdup`/`xAsprintf` map to owned
/// `String`/`format!`.
///
/// The goto-based cleanup (`context_error`/`ptrace_error`/`addr_space_error`) is
/// rendered as explicit cleanup-before-return in the same order the labels run:
/// `_UPT_destroy` → `ptraceDetach` → `unw_destroy_addr_space`.
pub fn UnwindPtrace_makeBacktrace(
    frames: &mut Vec<BacktraceFrameData>,
    pid: pid_t,
    error: &mut Option<String>,
) {
    // C: *error = NULL;
    *error = None;

    // C: if (pid <= 0) { *error = xStrdup("Invalid PID"); return; }
    if pid <= 0 {
        *error = Some("Invalid PID".to_string());
        return;
    }

    // C: unw_addr_space_t addrSpace = unw_create_addr_space(&_UPT_accessors, 0);
    let addr_space = unsafe {
        unw_create_addr_space(
            std::ptr::addr_of!(_UPT_accessors) as *mut unw_accessors_t,
            0,
        )
    };
    if addr_space.is_null() {
        *error = Some("Cannot initialize libunwind".to_string());
        return;
    }

    // C: int ptraceErrno = ptraceAttach(pid); if (ptraceErrno) goto addr_space_error;
    let ptrace_errno = ptraceAttach(pid);
    if ptrace_errno != 0 {
        *error = Some(format!(
            "ptrace: {} ({})",
            strerror(ptrace_errno),
            ptrace_errno
        ));
        unsafe { unw_destroy_addr_space(addr_space) }; // addr_space_error:
        return;
    }

    // C: int waitStatus = 0; if (wait(&waitStatus) == -1) goto ptrace_error;
    let mut wait_status: c_int = 0;
    if unsafe { libc::wait(&mut wait_status) } == -1 {
        let wait_errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
        *error = Some(format!("wait: {} ({})", strerror(wait_errno), wait_errno));
        ptraceDetach(pid); // ptrace_error:
        unsafe { unw_destroy_addr_space(addr_space) }; // addr_space_error:
        return;
    }

    // C: if (WIFSTOPPED(waitStatus) == 0) goto ptrace_error;
    if !libc::WIFSTOPPED(wait_status) {
        *error = Some("The process chosen is not stopped correctly".to_string());
        ptraceDetach(pid);
        unsafe { unw_destroy_addr_space(addr_space) };
        return;
    }

    // C: struct UPT_info* context = _UPT_create(pid); if (!context) goto ptrace_error;
    let context = unsafe { _UPT_create(pid) };
    if context.is_null() {
        *error = Some("Cannot create the context of libunwind-ptrace".to_string());
        ptraceDetach(pid);
        unsafe { unw_destroy_addr_space(addr_space) };
        return;
    }

    // C: unw_cursor_t cursor; int ret = unw_init_remote(&cursor, addrSpace, context);
    let mut cursor = unw_cursor_t {
        opaque: [0; CURSOR_LEN],
    };
    let ret = unsafe { unw_init_remote(&mut cursor, addr_space, context) };
    if ret < 0 {
        *error = Some(format!("libunwind cursor: ret={}", ret));
        // context_error:
        unsafe { _UPT_destroy(context) };
        ptraceDetach(pid);
        unsafe { unw_destroy_addr_space(addr_space) };
        return;
    }

    // C: unsigned int index = 0; do { … } while (unw_step(&cursor) > 0 && index < INT_MAX);
    let mut index: u32 = 0;
    loop {
        // C: char buffer[2048] = {0};
        let mut buffer = [0 as c_char; BUFFER_LEN];

        // C: BacktraceFrameData* frame = BacktraceFrameData_new(); frame->index = index;
        let mut frame = BacktraceFrameData_new();
        frame.index = index;

        // C: ret = unw_get_reg(&cursor, UNW_REG_IP, &pc);
        let mut pc: unw_word_t = 0;
        let ret = unsafe { unw_get_reg(&mut cursor, UNW_REG_IP, &mut pc) };
        if ret != 0 {
            *error = Some(format!(
                "Cannot get program counter register: error {}",
                -ret
            ));
            BacktraceFrameData_delete(frame);
            break;
        }
        // C: frame->address = pc;
        frame.address = pc as usize;

        // C: frame->isSignalFrame = unw_is_signal_frame(&cursor) > 0;
        frame.isSignalFrame = unsafe { unw_is_signal_frame(&mut cursor) } > 0;

        // C (HAVE_LIBUNWIND_ELF_FILENAME): if (unw_get_elf_filename(...) == 0)
        //     frame->objectPath = xStrndup(buffer, sizeof(buffer));
        // `unw_get_elf_filename` is present in current libunwind (declared in the
        // fetched `libunwind-common.h.in:298`), so it is always compiled here.
        let mut offset_elf_file_name: unw_word_t = 0;
        if unsafe {
            unw_get_elf_filename(
                &mut cursor,
                buffer.as_mut_ptr(),
                BUFFER_LEN,
                &mut offset_elf_file_name,
            )
        } == 0
        {
            // C: frame->objectPath = xStrndup(buffer, sizeof(buffer));
            // SAFETY: libunwind NUL-terminates within the 2048-byte `buffer`.
            frame.objectPath = Some(
                unsafe { CStr::from_ptr(buffer.as_ptr()) }
                    .to_string_lossy()
                    .into_owned(),
            );
        }

        // C: if (unw_get_proc_name(&cursor, buffer, sizeof(buffer), &offset) == 0) { … }
        let mut offset: unw_word_t = 0;
        if unsafe { unw_get_proc_name(&mut cursor, buffer.as_mut_ptr(), BUFFER_LEN, &mut offset) }
            == 0
        {
            // C: frame->offset = offset; frame->functionName = xStrndup(buffer, sizeof(buffer));
            frame.offset = offset as usize;
            // C: frame->functionName = xStrndup(buffer, sizeof(buffer));
            // SAFETY: libunwind NUL-terminates within the 2048-byte `buffer`.
            frame.functionName = Some(
                unsafe { CStr::from_ptr(buffer.as_ptr()) }
                    .to_string_lossy()
                    .into_owned(),
            );

            // C (HAVE_DEMANGLING): frame->demangleFunctionName = Demangle_demangle(frame->functionName);
            #[cfg(feature = "demangle")]
            if let Some(fname) = frame.functionName.as_deref() {
                if let Ok(cs) = std::ffi::CString::new(fname) {
                    frame.demangleFunctionName =
                        crate::ported::generic::demangle::Demangle_demangle(&cs);
                }
            }
        }

        // C: Vector_add(frames, (Object*)frame); index++;
        frames.push(frame);
        index += 1;

        // C: } while (unw_step(&cursor) > 0 && index < INT_MAX);
        if !(unsafe { unw_step(&mut cursor) } > 0 && index < i32::MAX as u32) {
            break;
        }
    }

    // context_error: / ptrace_error: / addr_space_error: (normal fall-through).
    unsafe { _UPT_destroy(context) };
    ptraceDetach(pid);
    unsafe { unw_destroy_addr_space(addr_space) };
}
