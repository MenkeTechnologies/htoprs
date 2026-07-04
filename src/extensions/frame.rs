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
use std::collections::BTreeMap;
use std::io::{self, Write};

thread_local! {
    /// The in-progress frame's bytes, or `None` when no frame is open.
    static FRAME: RefCell<Option<Vec<u8>>> = const { RefCell::new(None) };
    /// The last frame we presented, split into per-terminal-row byte segments,
    /// so [`present`] can skip re-emitting rows that did not change — the core
    /// of the flicker fix (a CPU-% tick repaints a handful of rows, not the
    /// whole ~150 KB screen). `None` until the first frame or after a clear.
    static PREV: RefCell<Option<Screen>> = const { RefCell::new(None) };
}

/// A presented frame decomposed for row-level diffing: a `preamble` (bytes
/// before the first cursor move — usually an initial SGR / cursor toggle) and
/// one self-contained byte segment per terminal row. Each row segment begins
/// with the SGR that was in effect when the cursor landed on the row, so it can
/// be re-emitted alone and render identically regardless of which other rows
/// were skipped.
#[derive(Default, PartialEq)]
struct Screen {
    preamble: Vec<u8>,
    rows: BTreeMap<usize, Vec<u8>>,
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

/// Drop the diff cache so the next [`present`] re-emits every row in full.
/// Call this whenever something painted the screen outside the frame pipeline
/// (a modal that draws directly and clears, a resize, an explicit redraw) —
/// otherwise the diff would compare against a stale picture and skip rows that
/// are actually different on screen.
pub fn invalidate() {
    PREV.with(|p| *p.borrow_mut() = None);
}

/// Close the open frame and present only what changed since the last frame:
/// parse the buffer into per-row segments, diff against the previous frame, and
/// write just the changed rows (wrapped in one 2026 region). A no-op when no
/// frame is open, the frame is empty, or nothing changed.
pub fn present() {
    let mut out = io::stdout().lock();
    if present_to(&mut out) {
        let _ = out.flush();
    }
}

/// Diff the open frame against the last one and write only the changed rows to
/// `out`, wrapped in `Begin`/`End`. Returns `true` if anything was written.
/// Split out from [`present`] so the diff is testable against an in-memory
/// writer. The frame is always closed (taken) if one was open.
fn present_to<W: Write>(out: &mut W) -> bool {
    let taken = FRAME.with(|f| f.borrow_mut().take());
    let Some(buf) = taken else {
        return false;
    };
    if buf.is_empty() {
        return false;
    }

    let (next, clear) = parse_frame(&buf);

    // A screen clear (`\e[2J`, from a modal) invalidates the whole prior frame.
    let base = PREV.with(|p| p.borrow_mut().take());
    let base = if clear { None } else { base };

    let mut body: Vec<u8> = Vec::new();

    // Preamble (initial SGR / cursor toggle): emit when it changed.
    if base.as_ref().map(|b| &b.preamble) != Some(&next.preamble) {
        body.extend_from_slice(&next.preamble);
    }

    // Changed rows only — this is what removes the full-screen repaint.
    for (row, seg) in &next.rows {
        if base.as_ref().and_then(|b| b.rows.get(row)) != Some(seg) {
            body.extend_from_slice(seg);
        }
    }

    // Rows that existed last frame but are absent now (terminal shrank / fewer
    // lines drawn): blank them so no stale text lingers.
    if let Some(b) = base.as_ref() {
        for row in b.rows.keys() {
            if !next.rows.contains_key(row) {
                let _ = write!(&mut body, "\x1b[{};1H\x1b[0m\x1b[K", row + 1);
            }
        }
    }

    PREV.with(|p| *p.borrow_mut() = Some(next));

    if body.is_empty() {
        return false; // nothing changed → nothing to draw → no flicker
    }
    let _ = out.write_all(BEGIN_SYNC);
    let _ = out.write_all(&body);
    let _ = out.write_all(END_SYNC);
    true
}

/// Split a frame's raw byte stream into a [`Screen`] for row diffing. Tracks the
/// cursor row from CUP (`\e[row;colH`) sequences and the active SGR state; each
/// row segment is prefixed with the entry SGR so it renders correctly on its
/// own. Also reports whether a full-screen clear (`\e[2J`) appeared. Any bytes
/// that are not a recognized CSI are copied through verbatim, so unknown
/// sequences and text are preserved.
fn parse_frame(frame: &[u8]) -> (Screen, bool) {
    let mut screen = Screen::default();
    let mut clear = false;
    let mut cur_row: Option<usize> = None;
    let mut cur_sgr: Vec<u8> = Vec::new();

    // Append `bytes` to whichever bucket the cursor is currently in.
    fn push(screen: &mut Screen, cur_row: Option<usize>, bytes: &[u8]) {
        match cur_row {
            None => screen.preamble.extend_from_slice(bytes),
            // The bucket exists: it is created when the cursor moves to the row.
            Some(r) => {
                if let Some(buf) = screen.rows.get_mut(&r) {
                    buf.extend_from_slice(bytes);
                }
            }
        }
    }

    let n = frame.len();
    let mut i = 0;
    while i < n {
        if frame[i] == 0x1b && i + 1 < n && frame[i + 1] == b'[' {
            // CSI: params (0x30..=0x3f), intermediates (0x20..=0x2f), final (0x40..=0x7e).
            let start = i;
            let mut j = i + 2;
            while j < n && (0x20..=0x3f).contains(&frame[j]) {
                j += 1;
            }
            if j >= n {
                // Truncated CSI at end of buffer — copy the rest through.
                push(&mut screen, cur_row, &frame[start..]);
                break;
            }
            let final_b = frame[j];
            let params = &frame[i + 2..j];
            let seq = &frame[start..=j];
            match final_b {
                b'H' | b'f' => {
                    let row = parse_first_param(params).saturating_sub(1);
                    cur_row = Some(row);
                    // New row → seed with the entry SGR so it's self-contained.
                    let bucket = screen.rows.entry(row).or_insert_with(|| cur_sgr.clone());
                    bucket.extend_from_slice(seq);
                }
                b'm' => {
                    if is_sgr_reset(params) {
                        cur_sgr.clear();
                    }
                    cur_sgr.extend_from_slice(seq);
                    push(&mut screen, cur_row, seq);
                }
                b'J' => {
                    if params == b"2" {
                        clear = true;
                    }
                    push(&mut screen, cur_row, seq);
                }
                _ => push(&mut screen, cur_row, seq),
            }
            i = j + 1;
        } else {
            // Text or a non-CSI escape: copy the single byte through.
            push(&mut screen, cur_row, &frame[i..i + 1]);
            i += 1;
        }
    }

    (screen, clear)
}

/// The first numeric parameter of a CSI sequence (digits before `;`), or `1`
/// when absent — matching the CUP default (`\e[H` == row 1, col 1).
fn parse_first_param(params: &[u8]) -> usize {
    let mut n = 0usize;
    let mut seen = false;
    for &b in params {
        if b.is_ascii_digit() {
            n = n * 10 + (b - b'0') as usize;
            seen = true;
        } else {
            break;
        }
    }
    if seen {
        n
    } else {
        1
    }
}

/// Whether an SGR sequence resets all attributes (`\e[m`, `\e[0m`, `\e[00m`).
fn is_sgr_reset(params: &[u8]) -> bool {
    params.is_empty() || params == b"0" || params == b"00"
}


#[cfg(test)]
mod tests {
    use super::*;

    /// Fresh thread-local state (each test thread starts clean, but be explicit).
    fn reset() {
        FRAME.with(|f| *f.borrow_mut() = None);
        PREV.with(|p| *p.borrow_mut() = None);
    }

    /// Draw `bytes` as one frame and present it to an in-memory sink; returns
    /// (sink bytes, wrote?).
    fn render(bytes: &[u8]) -> (Vec<u8>, bool) {
        begin_frame();
        frame_out().write_all(bytes).unwrap();
        let mut sink = Vec::new();
        let wrote = present_to(&mut sink);
        (sink, wrote)
    }

    /// Strip the 2026 begin/end wrapper to inspect the body.
    fn body(sink: &[u8]) -> Vec<u8> {
        assert!(sink.starts_with(BEGIN_SYNC) && sink.ends_with(END_SYNC));
        sink[BEGIN_SYNC.len()..sink.len() - END_SYNC.len()].to_vec()
    }

    /// The first frame is emitted in full (no previous frame to diff against),
    /// wrapped in exactly one 2026 region.
    #[test]
    fn first_frame_emits_everything() {
        reset();
        let (sink, wrote) = render(b"\x1b[1;1HAAAA\x1b[2;1HBBBB");
        assert!(wrote);
        assert_eq!(body(&sink), b"\x1b[1;1HAAAA\x1b[2;1HBBBB");
        assert_eq!(sink.windows(BEGIN_SYNC.len()).filter(|w| *w == BEGIN_SYNC).count(), 1);
        reset();
    }

    /// An identical second frame changes nothing → nothing is written. This is
    /// what stops the idle full-screen repaint from flickering.
    #[test]
    fn identical_frame_writes_nothing() {
        reset();
        let f = b"\x1b[1;1HAAAA\x1b[2;1HBBBB";
        assert!(render(f).1);
        let (sink, wrote) = render(f);
        assert!(!wrote, "unchanged frame must not emit");
        assert!(sink.is_empty());
        reset();
    }

    /// When one row changes, ONLY that row is re-emitted — not the whole screen.
    /// This is the core of the flicker fix: a CPU-% tick touches a few rows.
    #[test]
    fn only_changed_rows_are_emitted() {
        reset();
        assert!(render(b"\x1b[1;1HAAAA\x1b[2;1HBBBB\x1b[3;1HCCCC").1);
        // Row 2 (index 1) changes; rows 1 and 3 stay.
        let (sink, wrote) = render(b"\x1b[1;1HAAAA\x1b[2;1HZZZZ\x1b[3;1HCCCC");
        assert!(wrote);
        // Body is exactly the changed row's segment, nothing else.
        assert_eq!(body(&sink), b"\x1b[2;1HZZZZ");
        reset();
    }

    /// A row's colour, set by an SGR that preceded its cursor move, is carried
    /// as the row's entry SGR — so re-emitting only that row keeps the colour
    /// even though the SGR-setting bytes lived in an earlier (skipped) place.
    #[test]
    fn changed_row_keeps_its_entry_sgr() {
        reset();
        // Green set once up front, then two rows drawn in it.
        assert!(render(b"\x1b[32m\x1b[1;1HG1\x1b[2;1HG2").1);
        // Row 2 changes; the emitted segment must re-assert green.
        let (sink, _) = render(b"\x1b[32m\x1b[1;1HG1\x1b[2;1HXY");
        assert_eq!(body(&sink), b"\x1b[32m\x1b[2;1HXY");
        reset();
    }

    /// A `\e[2J` clear invalidates the whole previous frame → everything is
    /// re-emitted even if row bytes match.
    #[test]
    fn clear_forces_full_redraw() {
        reset();
        let f = b"\x1b[1;1HAAAA\x1b[2;1HBBBB";
        assert!(render(f).1);
        // Same rows, but prefixed with a clear: must re-emit both rows.
        let (sink, wrote) = render(b"\x1b[2J\x1b[1;1HAAAA\x1b[2;1HBBBB");
        assert!(wrote);
        assert!(body(&sink).windows(4).any(|w| w == b"AAAA"));
        assert!(body(&sink).windows(4).any(|w| w == b"BBBB"));
        reset();
    }

    /// A row present last frame but absent now is blanked (cleared to EOL) so
    /// no stale text is left behind when the drawn area shrinks.
    #[test]
    fn vanished_row_is_blanked() {
        reset();
        assert!(render(b"\x1b[1;1HAAAA\x1b[2;1HBBBB").1);
        // Second frame only draws row 1; row 2 disappears.
        let (sink, wrote) = render(b"\x1b[1;1HAAAA");
        assert!(wrote);
        // Body clears row 2 (1-based line 2).
        assert!(body(&sink).windows(6).any(|w| w == b"\x1b[2;1H"));
        assert!(body(&sink).windows(3).any(|w| w == b"\x1b[K"));
        reset();
    }

    // ── parser unit tests ──

    /// CUP row parsing is 1-based → 0-based; text lands in the right row bucket.
    #[test]
    fn parse_splits_rows_by_cup() {
        let (s, clear) = parse_frame(b"pre\x1b[3;1Hthird\x1b[1;5Hfirst");
        assert!(!clear);
        assert_eq!(s.preamble, b"pre");
        assert_eq!(s.rows.get(&2).map(|v| &v[..]), Some(&b"\x1b[3;1Hthird"[..]));
        assert_eq!(s.rows.get(&0).map(|v| &v[..]), Some(&b"\x1b[1;5Hfirst"[..]));
    }

    /// `\e[2J` sets the clear flag.
    #[test]
    fn parse_detects_clear() {
        let (_, clear) = parse_frame(b"\x1b[2J\x1b[1;1Hx");
        assert!(clear);
        let (_, clear2) = parse_frame(b"\x1b[0J\x1b[1;1Hx");
        assert!(!clear2);
    }

    /// A bare `\e[H` defaults to row 1 (index 0).
    #[test]
    fn parse_first_param_defaults_to_one() {
        assert_eq!(parse_first_param(b""), 1);
        assert_eq!(parse_first_param(b"12;34"), 12);
        assert_eq!(parse_first_param(b"7"), 7);
    }

    /// SGR reset detection.
    #[test]
    fn sgr_reset_recognized() {
        assert!(is_sgr_reset(b""));
        assert!(is_sgr_reset(b"0"));
        assert!(!is_sgr_reset(b"1"));
        assert!(!is_sgr_reset(b"38;5;2"));
    }

    /// A truncated CSI at end of buffer is copied through, not dropped/panicked.
    #[test]
    fn truncated_csi_is_preserved() {
        let (s, _) = parse_frame(b"\x1b[1;1Hx\x1b[3");
        assert_eq!(s.rows.get(&0).map(|v| &v[..]), Some(&b"\x1b[1;1Hx\x1b[3"[..]));
    }

    /// Frame buffering: writes accumulate and `begin_frame` clears stale bytes.
    #[test]
    fn begin_frame_buffers_and_clears() {
        reset();
        begin_frame();
        frame_out().write_all(b"stale").unwrap();
        begin_frame();
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
        reset();
    }
}
