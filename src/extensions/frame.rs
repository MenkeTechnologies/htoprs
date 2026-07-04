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
    let mut out = io::stdout().lock();
    if present_to(&mut out) {
        let _ = out.flush();
    }
}

/// Take the open frame and, if non-empty, write `Begin` + bytes + `End` to
/// `out` in that order. Returns `true` if anything was written. The frame is
/// always closed (taken) if one was open, empty or not. Split out from
/// [`present`] so the wrapping/atomicity contract is testable against an
/// in-memory writer without touching the real terminal.
fn present_to<W: Write>(out: &mut W) -> bool {
    let taken = FRAME.with(|f| f.borrow_mut().take());
    match taken {
        Some(buf) if !buf.is_empty() => {
            let _ = out.write_all(BEGIN_SYNC);
            let _ = out.write_all(&buf);
            let _ = out.write_all(END_SYNC);
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each test runs on its own thread → fresh (`None`) thread-local FRAME.
    /// Belt-and-suspenders: close any stray frame before asserting.
    fn reset() {
        FRAME.with(|f| *f.borrow_mut() = None);
    }

    /// While a frame is open, writes are buffered (not sent) and `present_to`
    /// wraps them in the 2026 begin/end sequences in a single burst.
    #[test]
    fn frame_buffers_then_present_wraps_atomically() {
        reset();
        begin_frame();
        let mut o = frame_out();
        o.write_all(b"hello").unwrap();
        o.flush().unwrap(); // must be a no-op while the frame is open
        // Nothing is emitted until present; the buffer still holds the bytes.
        FRAME.with(|f| assert_eq!(f.borrow().as_deref(), Some(&b"hello"[..])));

        let mut sink = Vec::new();
        assert!(present_to(&mut sink));
        // Exact bytes: Begin + content + End, in one contiguous region.
        assert_eq!(sink, b"\x1b[?2026hhello\x1b[?2026l");
        // Frame is closed afterward.
        FRAME.with(|f| assert!(f.borrow().is_none()));
    }

    /// The begin sequence precedes and the end sequence follows ALL content —
    /// nothing leaks outside the synchronized-update region.
    #[test]
    fn content_is_fully_enclosed_by_sync_region() {
        reset();
        begin_frame();
        // Simulate several draw calls (header, rows, function bar, overlay).
        for chunk in [&b"HEADER"[..], b"row1", b"row2", b"FnBar", b"toast"] {
            frame_out().write_all(chunk).unwrap();
        }
        let mut sink = Vec::new();
        present_to(&mut sink);
        let begin = sink.windows(BEGIN_SYNC.len()).position(|w| w == BEGIN_SYNC);
        let end = sink.windows(END_SYNC.len()).position(|w| w == END_SYNC);
        assert_eq!(begin, Some(0), "Begin must be at the very start");
        // End is the last thing written.
        assert_eq!(end, Some(sink.len() - END_SYNC.len()));
        // The concatenated content sits between them, in draw order.
        let inner = &sink[BEGIN_SYNC.len()..sink.len() - END_SYNC.len()];
        assert_eq!(inner, b"HEADERrow1row2FnBartoast");
        // And there is only ONE begin and ONE end (single atomic region).
        assert_eq!(sink.windows(BEGIN_SYNC.len()).filter(|w| *w == BEGIN_SYNC).count(), 1);
        assert_eq!(sink.windows(END_SYNC.len()).filter(|w| *w == END_SYNC).count(), 1);
    }

    /// Multiple `frame_out()` handles append to the same buffer in call order.
    #[test]
    fn multiple_writes_accumulate_in_order() {
        reset();
        begin_frame();
        frame_out().write_all(b"a").unwrap();
        frame_out().write_all(b"bc").unwrap();
        frame_out().write_all(b"def").unwrap();
        FRAME.with(|f| assert_eq!(f.borrow().as_deref(), Some(&b"abcdef"[..])));
        reset();
    }

    /// `write` reports the full length consumed (buffered path).
    #[test]
    fn write_reports_full_length() {
        reset();
        begin_frame();
        let n = frame_out().write(b"twelve bytes").unwrap();
        assert_eq!(n, 12);
        reset();
    }

    /// An empty frame presents nothing — no stray 2026 codes on a no-op redraw.
    #[test]
    fn empty_frame_emits_nothing() {
        reset();
        begin_frame(); // opened but never written to
        let mut sink = Vec::new();
        assert!(!present_to(&mut sink));
        assert!(sink.is_empty());
        // Frame is still closed (taken) even though empty.
        FRAME.with(|f| assert!(f.borrow().is_none()));
    }

    /// `begin_frame` clears any un-presented bytes (reused allocation).
    #[test]
    fn begin_frame_clears_stale_bytes() {
        reset();
        begin_frame();
        frame_out().write_all(b"stale").unwrap();
        begin_frame(); // should clear, not append
        FRAME.with(|f| assert_eq!(f.borrow().as_deref(), Some(&b""[..])));
        reset();
    }

    /// `present_to` with no open frame writes nothing and returns false.
    #[test]
    fn present_without_frame_is_noop() {
        reset();
        let mut sink = Vec::new();
        assert!(!present_to(&mut sink));
        assert!(sink.is_empty());
        FRAME.with(|f| assert!(f.borrow().is_none()));
    }

    /// A large frame (many small writes, as a full process list produces)
    /// accumulates fully and presents as one region — the load-bearing case for
    /// the flicker fix, since a big slow frame is exactly what tripped the old
    /// incremental-flush timeout.
    #[test]
    fn large_frame_accumulates_and_presents_once() {
        reset();
        begin_frame();
        let mut expected = Vec::new();
        for i in 0..5000u32 {
            let cell = format!("\x1b[38;5;{}m#", i % 256);
            frame_out().write_all(cell.as_bytes()).unwrap();
            expected.extend_from_slice(cell.as_bytes());
        }
        let mut sink = Vec::new();
        assert!(present_to(&mut sink));
        assert_eq!(&sink[..BEGIN_SYNC.len()], BEGIN_SYNC);
        assert_eq!(&sink[sink.len() - END_SYNC.len()..], END_SYNC);
        assert_eq!(&sink[BEGIN_SYNC.len()..sink.len() - END_SYNC.len()], &expected[..]);
        reset();
    }

    /// `flush()` inside an open frame is a no-op (does not close or present it);
    /// the buffered bytes survive to the eventual `present`.
    #[test]
    fn flush_during_frame_does_not_present() {
        reset();
        begin_frame();
        let mut o = frame_out();
        o.write_all(b"kept").unwrap();
        o.flush().unwrap();
        o.flush().unwrap();
        FRAME.with(|f| assert_eq!(f.borrow().as_deref(), Some(&b"kept"[..])));
        let mut sink = Vec::new();
        present_to(&mut sink);
        assert_eq!(sink, b"\x1b[?2026hkept\x1b[?2026l");
    }
}
