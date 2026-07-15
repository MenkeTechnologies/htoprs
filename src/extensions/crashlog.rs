//! htoprs-original crash logging (no htop C analog).
//!
//! A TUI process viewer spends its life on the alternate screen with the tty in
//! raw mode, so a crash report written to `stderr` is painted over and lost the
//! moment the terminal is restored. This module persists crash detail to a file
//! the user can read after the fact:
//!
//! - [`log_panic`] is called from the process-wide panic hook installed in
//!   [`crate::ported::commandline::CommandLine_run`]. It records the panic
//!   message, source location, thread name, htoprs version, and a full
//!   backtrace (captured via [`std::backtrace::Backtrace::force_capture`], so a
//!   backtrace is always present regardless of `RUST_BACKTRACE`).
//! - [`log_line`] appends an arbitrary pre-formatted crash report (used to tee
//!   the ported SIGSEGV/SIGBUS handler's stderr report into the same file).
//!
//! The log path is `$XDG_CACHE_HOME/htoprs/crash.log`, falling back to
//! `$HOME/.cache/htoprs/crash.log` (mirroring the wider MenkeTechnologies
//! convention of `~/.cache/<tool>/<tool>.log`), or `$CRASH_LOG` verbatim when
//! that env var is set. Entries are appended, newest last, so a run history is
//! preserved across crashes.
//!
//! Every operation here is best-effort and must never panic: this code runs
//! *inside* the panic hook, and a panic during unwinding aborts the process.

use std::backtrace::Backtrace;
use std::cell::RefCell;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::panic::PanicHookInfo;
use std::path::PathBuf;

/// Resolve the crash-log file path. Honors `$CRASH_LOG` verbatim, else
/// `$XDG_CACHE_HOME/htoprs/crash.log`, else `$HOME/.cache/htoprs/crash.log`.
/// Returns `None` only when neither `$CRASH_LOG`, `$XDG_CACHE_HOME`, nor
/// `$HOME` is set (a headless environment with no writable home).
pub fn crash_log_path() -> Option<PathBuf> {
    if let Ok(explicit) = std::env::var("CRASH_LOG") {
        if !explicit.is_empty() {
            return Some(PathBuf::from(explicit));
        }
    }
    let dir = if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
        if xdg.is_empty() {
            return home_cache_dir();
        }
        PathBuf::from(xdg).join("htoprs")
    } else {
        return home_cache_dir();
    };
    Some(dir.join("crash.log"))
}

fn home_cache_dir() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok().filter(|h| !h.is_empty())?;
    Some(PathBuf::from(home).join(".cache/htoprs/crash.log"))
}

/// `strftime("%F %H:%M:%S")` of the current local time, matching the timestamp
/// style used by the ported DateTimeMeter (`localtime_r` + `strftime`). Falls
/// back to the raw epoch-seconds count if `strftime` yields nothing.
fn timestamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as libc::time_t)
        .unwrap_or(0);
    // SAFETY: localtime_r writes into the zeroed `tm`; strftime writes at most
    // `buf.len()` bytes and returns the count written (0 on overflow).
    let mut result: libc::tm = unsafe { std::mem::zeroed() };
    let mut buf = [0u8; 64];
    let n = unsafe {
        libc::localtime_r(&secs, &mut result);
        let fmt = c"%F %H:%M:%S";
        libc::strftime(
            buf.as_mut_ptr() as *mut libc::c_char,
            buf.len(),
            fmt.as_ptr(),
            &result,
        )
    };
    if n == 0 {
        return format!("epoch:{secs}");
    }
    String::from_utf8_lossy(&buf[..n]).into_owned()
}

/// Format and append a panic to the crash log. Returns the path written on
/// success so the caller can point the user at it; `None` if no log path could
/// be resolved or the file could not be written.
pub fn log_panic(info: &PanicHookInfo<'_>) -> Option<PathBuf> {
    let payload = info.payload();
    let message = if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "Box<dyn Any>".to_string()
    };
    let location = info
        .location()
        .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
        .unwrap_or_else(|| "<unknown location>".to_string());
    let thread = std::thread::current()
        .name()
        .unwrap_or("<unnamed>")
        .to_string();
    let backtrace = Backtrace::force_capture();

    let entry = format!(
        "\n\
         ==================== htoprs crash ====================\n\
         time:      {ts}\n\
         version:   {version}\n\
         pid:       {pid}\n\
         thread:    {thread}\n\
         location:  {location}\n\
         message:   {message}\n\
         --- backtrace ---\n\
         {backtrace}\n\
         ======================================================\n",
        ts = timestamp(),
        version = env!("CARGO_PKG_VERSION"),
        pid = std::process::id(),
    );
    log_line(&entry)
}

// The reason the run loop last recorded for its imminent termination, set by
// `set_exit_reason` and drained by `flush_exit`. Thread-local because the TUI
// reads keys and runs the loop on a single thread (`ScreenManager_run`); nested
// screens (Setup, the process-kill list) overwrite it harmlessly, so the value
// present when the top-level loop unwinds is the true exit cause.
thread_local! {
    static PENDING_EXIT: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Record why the current run loop is about to unwind toward process exit
/// (a quit keystroke, stdin EOF, or the `-n` iteration limit). The last reason
/// set before the top-level `ScreenManager_run` returns is the one [`flush_exit`]
/// writes. Cheap and allocation-light on the hot path — only called at the
/// single frame where the loop decides to break.
pub fn set_exit_reason(reason: impl Into<String>) {
    PENDING_EXIT.with(|c| *c.borrow_mut() = Some(reason.into()));
}

/// Write the pending exit reason recorded by [`set_exit_reason`] to the log,
/// then clear it. Called once at true program exit (after the top-level run
/// loop returns). A no-op returning `None` when nothing was recorded — e.g. a
/// signal-driven exit, which never reaches this path and logs itself from the
/// signal handler via [`log_exit`].
pub fn flush_exit() -> Option<PathBuf> {
    let reason = PENDING_EXIT.with(|c| c.borrow_mut().take())?;
    log_exit(&reason)
}

/// Append a terminal "exit" record (timestamp, version, pid, reason) to the
/// same log as [`log_panic`]. Used both for clean exits (via [`flush_exit`])
/// and directly from the signal handlers, so every way htoprs can leave the
/// run loop lands one line in `crash.log` explaining why. Returns the path
/// written, or `None` if no log path resolves / the write fails.
pub fn log_exit(reason: &str) -> Option<PathBuf> {
    let entry = format!(
        "\n\
         -------------------- htoprs exit ---------------------\n\
         time:      {ts}\n\
         version:   {version}\n\
         pid:       {pid}\n\
         reason:    {reason}\n\
         ------------------------------------------------------\n",
        ts = timestamp(),
        version = env!("CARGO_PKG_VERSION"),
        pid = std::process::id(),
    );
    log_line(&entry)
}

/// Append a pre-formatted report to the crash log. Best-effort: creates the
/// parent directory, opens the file for append, and writes. Returns the path on
/// success, `None` on any failure (no path resolvable, mkdir/open/write error).
pub fn log_line(text: &str) -> Option<PathBuf> {
    let path = crash_log_path()?;
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .ok()?;
    file.write_all(text.as_bytes()).ok()?;
    let _ = file.flush();
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `$CRASH_LOG` overrides every other source and is honored verbatim, and
    /// [`log_line`] appends (newest last) rather than truncating — so a run's
    /// crash history survives across crashes.
    #[test]
    fn crash_log_appends_to_explicit_path() {
        let dir = std::env::temp_dir().join(format!("htoprs-crashlog-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let log = dir.join("crash.log");
        let _ = fs::remove_file(&log);
        // SAFETY: single-threaded within this test; no other test reads CRASH_LOG.
        unsafe {
            std::env::set_var("CRASH_LOG", &log);
        }

        assert_eq!(crash_log_path().as_deref(), Some(log.as_path()));

        let p1 = log_line("first\n").expect("write 1");
        let p2 = log_line("second\n").expect("write 2");
        assert_eq!(p1, log);
        assert_eq!(p2, log);

        let contents = fs::read_to_string(&log).expect("read back");
        assert_eq!(contents, "first\nsecond\n", "append, not truncate");

        // SAFETY: single-threaded teardown.
        unsafe {
            std::env::remove_var("CRASH_LOG");
        }
        let _ = fs::remove_dir_all(&dir);
    }

    /// The timestamp is a non-empty `strftime`-style string (never the empty
    /// buffer), so every log entry carries a readable time field.
    #[test]
    fn timestamp_is_non_empty() {
        let ts = timestamp();
        assert!(!ts.is_empty());
        // "%F %H:%M:%S" → "YYYY-MM-DD HH:MM:SS" is 19 chars; the epoch fallback
        // ("epoch:N") is shorter but still non-empty. Guard the common path.
        assert!(ts.len() >= 7, "unexpectedly short timestamp: {ts:?}");
    }
}
