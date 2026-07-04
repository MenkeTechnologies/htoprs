//! htoprs-original: single-write frame presentation to kill terminal flicker.
//!
//! htop relies on ncurses' `doupdate` (diff against a virtual screen, emit only
//! changed cells). htoprs draws directly on crossterm, so the run loop repaints
//! every cell each frame. Wrapping that in a DEC-2026 synchronized-update region
//! (`\e[?2026h … \e[?2026l`) makes it atomic — but only if the terminal sees
//! `End` before its synchronized-update *timeout*. The draw code flushes to
//! stdout incrementally, so on a slow machine the bytes trickle out over the
//! whole (multi-millisecond) repaint; the terminal times out, auto-ends the
//! region, and renders a partial frame — visible flicker. Same terminal, faster
//! machine: the repaint finishes inside the timeout, so it never showed.
//!
//! The fix: collect the entire frame into a thread-local buffer during drawing
//! (no stdout writes, no flushes), then in [`present`] emit
//! `Begin` + the whole buffer + `End` in one `write_all`. Now `Begin` and `End`
//! reach the terminal microseconds apart regardless of how long the *drawing*
//! took, so the timeout can never trip; the slow compute happens before `Begin`
//! is ever sent. It also collapses many small writes into one, which by itself
//! removes most of the flicker even on terminals that ignore 2026.
//!
//! Drawing routes through [`frame_out`], a zero-sized [`Write`] that appends to
//! the buffer while a frame is open and falls back to real stdout otherwise —
//! so a draw done outside a `begin_frame`/`present` bracket (e.g. a modal that
//! paints and waits) still reaches the screen.

use std::cell::RefCell;
use std::io::{self, Write};

thread_local! {
    /// The in-progress frame's bytes, or `None` when no frame is open.
    static FRAME: RefCell<Option<Vec<u8>>> = const { RefCell::new(None) };
}

/// DEC private mode 2026 (synchronized update) begin/end.
const BEGIN_SYNC: &[u8] = b"\x1b[?2026h";
const END_SYNC: &[u8] = b"\x1b[?2026l";

/// Open a frame: subsequent [`frame_out`] writes are buffered until [`present`].
/// Idempotent — reusing the allocation and clearing any un-presented bytes.
pub fn begin_frame() {
    FRAME.with(|f| {
        let mut slot = f.borrow_mut();
        match slot.as_mut() {
            Some(buf) => buf.clear(),
            None => *slot = Some(Vec::with_capacity(64 * 1024)),
        }
    });
}

/// A [`Write`] sink for all frame drawing. While a frame is open it appends to
/// the thread-local buffer; otherwise it writes straight to stdout (so draws
/// outside a frame still show). Zero-sized: cheap to construct per draw call.
pub struct FrameOut;

impl Write for FrameOut {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        FRAME.with(|f| {
            let mut slot = f.borrow_mut();
            match slot.as_mut() {
                Some(buf) => {
                    buf.extend_from_slice(data);
                    Ok(data.len())
                }
                None => io::stdout().lock().write(data),
            }
        })
    }

    fn flush(&mut self) -> io::Result<()> {
        // A frame is flushed atomically in `present`; an incremental flush mid
        // frame is exactly what causes the tearing, so swallow it. Outside a
        // frame, forward the flush so direct draws land immediately.
        FRAME.with(|f| {
            if f.borrow().is_none() {
                io::stdout().lock().flush()
            } else {
                Ok(())
            }
        })
    }
}

/// The frame sink. Draw code uses this in place of `io::stdout().lock()`.
pub fn frame_out() -> FrameOut {
    FrameOut
}

/// Close the open frame and present it atomically: one `write_all` of
/// `Begin` + buffered bytes + `End`, then a single flush. A no-op when no frame
/// is open or the frame is empty.
pub fn present() {
    FRAME.with(|f| {
        let taken = f.borrow_mut().take();
        if let Some(buf) = taken {
            if !buf.is_empty() {
                let mut out = io::stdout().lock();
                let _ = out.write_all(BEGIN_SYNC);
                let _ = out.write_all(&buf);
                let _ = out.write_all(END_SYNC);
                let _ = out.flush();
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// While a frame is open, writes are buffered (not sent) and `present`
    /// wraps them in the 2026 begin/end sequences in a single burst.
    #[test]
    fn frame_buffers_then_present_wraps_atomically() {
        begin_frame();
        let mut o = frame_out();
        o.write_all(b"hello").unwrap();
        o.flush().unwrap(); // must be a no-op while the frame is open
        // Nothing is emitted until present; the buffer still holds the bytes.
        FRAME.with(|f| {
            assert_eq!(f.borrow().as_deref(), Some(&b"hello"[..]));
        });
        present();
        // After present the frame is closed.
        FRAME.with(|f| assert!(f.borrow().is_none()));
    }

    /// `begin_frame` clears any un-presented bytes (reused allocation).
    #[test]
    fn begin_frame_clears_stale_bytes() {
        begin_frame();
        frame_out().write_all(b"stale").unwrap();
        begin_frame(); // should clear, not append
        FRAME.with(|f| assert_eq!(f.borrow().as_deref(), Some(&b""[..])));
        present();
    }

    /// `present` with no open frame does nothing (no panic, no take).
    #[test]
    fn present_without_frame_is_noop() {
        // Ensure closed first.
        present();
        present();
        FRAME.with(|f| assert!(f.borrow().is_none()));
    }
}
