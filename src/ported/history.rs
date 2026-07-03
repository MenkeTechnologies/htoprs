//! Port of `History.c` — htop's LineEditor command-history ring buffer.
//!
//! This is the *top-level* `History.c` (the one whose `History.h`
//! includes `LineEditor.h`), i.e. the command-history ring used by the
//! incremental line editor, NOT any screen/graph history. The struct is
//! an oldest-first array of strings capped at `HISTORY_MAX_ENTRIES`,
//! with a browse `position` (`count` == "at new input") and a `saved`
//! scratch buffer for the in-progress input while browsing.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module — matching the spec
//! name-for-name is the point of the port. Each C function takes
//! `History* this`; the faithful analog is a free fn taking
//! `this: &mut History` / `this: &History` (the same shape `Vector.c`'s
//! port uses: free fns, not methods).
//!
//! Ported (self-contained, no unported substrate):
//! - `History_new` (`History.c:43`) — allocates the ring (capacity 64),
//!   loads from file if a filename is given, then parks `position` at
//!   `count`. It is the spec for the struct's initial state, so porting
//!   it is how the struct is "modeled faithfully".
//! - `History_load` (`History.c:22`) — `static` in C; reads the history
//!   file line by line, strips trailing `\n`/`\r`, skips empty lines,
//!   and feeds each line through `History_add`. Only reachable from
//!   `History_new` in C.
//! - `History_save` (`History.c:68`) — writes the (tail of the) ring to
//!   the history file, one entry per line, with `0600` perms.
//! - `History_add` (`History.c:86`) — dedup + grow/rotate + append.
//! - `History_resetPosition` (`History.c:149`) — parks the browse
//!   cursor at "new input" and clears `saved`.
//!
//! Ported (needs the now-ported `LineEditor`):
//! - `History_navigate` (`History.c:120`) — moves the browse cursor
//!   up/down the ring, saving the editor's current text into `saved`
//!   when first entering history and restoring it when the cursor
//!   returns to "new input". Uses `LineEditor_getText`
//!   (`LineEditor.h:37`), which now lives in the ported `lineeditor`
//!   module.
//!
//! Stubbed (deliberate non-port):
//! - `History_delete` (`History.c:60`) — frees the heap array, the
//!   `filename` string, and the struct itself. There is no faithful
//!   safe-Rust analog: `History` owns its `Vec<String>`/`String`
//!   fields, so `Drop` frees them automatically. A hand-written
//!   free-everything routine has nothing to do.
//!
//! Not replicated: the C reader uses a fixed `char line[LINEEDITOR_MAX +
//! 2]` (130-byte) `fgets` buffer, which would split any file line longer
//! than 129 bytes into multiple history entries. History strings
//! originate from the line editor and never exceed `LINEEDITOR_MAX`, so
//! that split is unreachable in practice; the task frames load/save as
//! "read/write lines", and reading whole lines is the faithful analog of
//! that intent. `String_eq(a, b)` (`XUtils.h:60`, `strcmp(a,b) == 0`) is
//! inlined as Rust `==` on the strings.
#![allow(non_snake_case)]
#![allow(dead_code)]

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::os::unix::fs::OpenOptionsExt;

use crate::ported::lineeditor::{LineEditor, LineEditor_getText};

/// Port of `#define HISTORY_MAX_ENTRIES 512` from `History.h`.
const HISTORY_MAX_ENTRIES: usize = 512;

/// Port of `#define LINEEDITOR_MAX 128` from `LineEditor.h:14`. Sizes
/// the `saved` scratch buffer (`char saved[LINEEDITOR_MAX + 1]`) and the
/// C reader's line buffer.
const LINEEDITOR_MAX: usize = 128;

/// Port of `struct History_` from `History.h`. `entries` is the
/// oldest-first ring (the C `char** entries`); `count` and `capacity`
/// mirror the C bookkeeping — `capacity` is tracked explicitly because
/// the grow-vs-rotate branch in `History_add` keys off it. The struct
/// owns its strings, so the C `char** entries` / `char* filename` heap
/// blocks become `Vec<String>` / `Option<String>` and free themselves.
pub struct History {
    /// C `char** entries` — history strings, oldest first. Invariant:
    /// `entries.len() == count`.
    pub entries: Vec<String>,
    /// C `size_t count` — current number of entries.
    pub count: usize,
    /// C `size_t capacity` — allocated capacity; gates grow vs rotate.
    pub capacity: usize,
    /// C `size_t position` — browse cursor; `count` == "at new input".
    pub position: usize,
    /// C `char saved[LINEEDITOR_MAX + 1]` — saved current input while
    /// browsing. Modeled as an owned `String`.
    pub saved: String,
    /// C `char* filename` — history file path (`None` == no read/write).
    pub filename: Option<String>,
}

/// Port of `static void History_load(History* this)` from
/// `History.c:22`. Reads the history file line by line, strips trailing
/// `\n`/`\r` (the C `while` loop strips both, handling `\r\n`), skips
/// empty lines, and adds each remaining line via `History_add`. Returns
/// early (no-op) when `filename` is `None` or the file cannot be opened,
/// exactly like the C `!this->filename` / `!fp` guards.
pub fn History_load(this: &mut History) {
    // Clone the path so `this` is free to be borrowed mutably by
    // `History_add` inside the loop.
    let filename = match &this.filename {
        Some(f) => f.clone(),
        None => return,
    };

    let file = match File::open(&filename) {
        Ok(f) => f,
        Err(_) => return,
    };

    let reader = BufReader::new(file);
    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        // strip trailing newline / carriage return
        let line = line.trim_end_matches(|c| c == '\n' || c == '\r');
        if line.is_empty() {
            continue;
        }

        History_add(this, line);
    }
}

/// Port of `History* History_new(const char* filename)` from
/// `History.c:43`. Allocates the ring with `capacity == 64`, an empty
/// `saved`, and the (optional) filename; loads from the file if a
/// filename was given; then parks `position` at `count` so browsing
/// starts at "new input".
pub fn History_new(filename: Option<&str>) -> History {
    let mut this = History {
        entries: Vec::with_capacity(64),
        count: 0,
        capacity: 64,
        position: 0,
        saved: String::new(),
        filename: filename.map(|s| s.to_string()),
    };

    if this.filename.is_some() {
        History_load(&mut this);
    }

    this.position = this.count;

    this
}

/// Port of `void History_delete(History* this)` from `History.c:60`. The C
/// frees each entry, the `entries` array, the `filename`, and the struct.
/// Taking `this` by value is the faithful analog of that `free` chain: the
/// moved-in [`History`] — and its owned `Vec<String>` `entries` and
/// `Option<String>` `filename` — drops at end of scope, which *is* the C
/// free sequence (the same by-value-consume idiom as `FunctionBar_delete`).
pub fn History_delete(this: History) {
    let _ = this;
}

/// Port of `void History_save(const History* this)` from
/// `History.c:68`. Writes the tail of the ring — from `start` to `count`
/// where `start = count > HISTORY_MAX_ENTRIES ? count - HISTORY_MAX_ENTRIES : 0`
/// — one entry per line. Opens with `O_WRONLY | O_CREAT | O_TRUNC` and
/// mode `0600`, matching the C `open(...)`; returns early (no-op) when
/// `filename` is `None` or the open fails.
pub fn History_save(this: &History) {
    let filename = match &this.filename {
        Some(f) => f,
        None => return,
    };

    let file = match OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(filename)
    {
        Ok(f) => f,
        Err(_) => return,
    };

    let mut fp = BufWriter::new(file);
    let start = if this.count > HISTORY_MAX_ENTRIES {
        this.count - HISTORY_MAX_ENTRIES
    } else {
        0
    };
    for i in start..this.count {
        // fprintf(fp, "%s\n", ...) — write errors are ignored, as in C.
        let _ = writeln!(fp, "{}", this.entries[i]);
    }
    let _ = fp.flush();
}

/// Port of `void History_add(History* this, const char* entry)` from
/// `History.c:86`. No-op for an empty entry. Removes a prior identical
/// entry if present (only the first match — the C loop `break`s), grows
/// the capacity by doubling up to `HISTORY_MAX_ENTRIES` or, once at that
/// cap, drops the oldest entry, then appends the new entry and resets
/// the browse position to "new input". `String_eq` (`strcmp == 0`) is
/// inlined as `==`; `xReallocArray`'s capacity growth has no observable
/// effect beyond the tracked `capacity` field, which gates the
/// grow-vs-rotate branch.
pub fn History_add(this: &mut History, entry: &str) {
    if entry.is_empty() {
        return;
    }

    // Deduplicate: remove previous identical entry if present.
    for i in 0..this.count {
        if this.entries[i] == entry {
            this.entries.remove(i);
            this.count -= 1;
            break;
        }
    }

    // Grow array if needed.
    if this.count >= this.capacity {
        if this.capacity < HISTORY_MAX_ENTRIES {
            this.capacity = (this.capacity * 2).min(HISTORY_MAX_ENTRIES);
        } else {
            // Drop oldest entry.
            this.entries.remove(0);
            this.count -= 1;
        }
    }

    this.entries.push(entry.to_string());
    this.count += 1;

    // Reset position to "at new input".
    this.position = this.count;
    this.saved.clear();
}

/// Port of `const char* History_navigate(History* this,
/// LineEditor* editor, bool back)` from `History.c:120`. Returns `None`
/// on an empty ring. Going `back` (up arrow): the first step out of "new
/// input" saves the editor's current text into `saved` (C
/// `strncpy(this->saved, LineEditor_getText(editor), LINEEDITOR_MAX)`
/// then NUL-terminates at `LINEEDITOR_MAX`), then the cursor walks toward
/// the oldest entry, returning `None` once already at the oldest. Going
/// forward (down arrow): `None` when already at the newest, otherwise the
/// cursor advances and — when it returns to "new input" — restores the
/// `saved` text, else returns the entry at the new position. The returned
/// `&str` borrows from `this` (either an entry or `saved`), matching the
/// C `const char*` aliasing into the struct.
pub fn History_navigate<'a>(
    this: &'a mut History,
    editor: &LineEditor,
    back: bool,
) -> Option<&'a str> {
    if this.count == 0 {
        return None;
    }

    if back {
        // Going back (up arrow)
        if this.position == this.count {
            // Save current editor content before entering history.
            // strncpy(..., LINEEDITOR_MAX) copies at most LINEEDITOR_MAX
            // bytes; the buffer is then NUL-terminated at LINEEDITOR_MAX.
            let text = LineEditor_getText(editor);
            let mut end = text.len().min(LINEEDITOR_MAX);
            while end > 0 && !text.is_char_boundary(end) {
                end -= 1;
            }
            this.saved.clear();
            this.saved.push_str(&text[..end]);
        }
        if this.position > 0 {
            this.position -= 1;
            return Some(&this.entries[this.position]);
        }
        None // Already at oldest entry
    } else {
        // Going forward (down arrow)
        if this.position >= this.count {
            return None; // Already at newest
        }
        this.position += 1;
        if this.position == this.count {
            // Restore saved input
            return Some(&this.saved);
        }
        Some(&this.entries[this.position])
    }
}

/// Port of `void History_resetPosition(History* this)` from
/// `History.c:149`. Parks the browse cursor at "new input"
/// (`position = count`) and clears the `saved` scratch buffer
/// (`saved[0] = '\0'`).
pub fn History_resetPosition(this: &mut History) {
    this.position = this.count;
    this.saved.clear();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    /// Unique, self-cleaning temp path for file-I/O tests (headless-safe).
    fn temp_path(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut p = std::env::temp_dir();
        p.push(format!(
            "htoprs_history_{}_{}_{}_{}",
            std::process::id(),
            tag,
            nanos,
            n
        ));
        p
    }

    #[test]
    fn new_none_defaults() {
        let h = History_new(None);
        assert!(h.entries.is_empty());
        assert_eq!(h.count, 0);
        assert_eq!(h.capacity, 64);
        assert_eq!(h.position, 0);
        assert!(h.saved.is_empty());
        assert!(h.filename.is_none());
    }

    #[test]
    fn add_appends_and_parks_position() {
        let mut h = History_new(None);
        h.position = 0; // pretend we were browsing
        h.saved.push_str("in progress");
        History_add(&mut h, "one");
        History_add(&mut h, "two");
        assert_eq!(h.entries, vec!["one".to_string(), "two".to_string()]);
        assert_eq!(h.count, 2);
        assert_eq!(h.position, 2); // parked at "new input"
        assert!(h.saved.is_empty()); // cleared on add
    }

    #[test]
    fn add_ignores_empty_entry() {
        let mut h = History_new(None);
        History_add(&mut h, "");
        assert_eq!(h.count, 0);
        assert!(h.entries.is_empty());
    }

    #[test]
    fn add_dedups_and_moves_to_end() {
        let mut h = History_new(None);
        History_add(&mut h, "a");
        History_add(&mut h, "b");
        History_add(&mut h, "c");
        // Re-adding "a" removes the old "a" and appends it at the end.
        History_add(&mut h, "a");
        assert_eq!(
            h.entries,
            vec!["b".to_string(), "c".to_string(), "a".to_string()]
        );
        assert_eq!(h.count, 3);
    }

    #[test]
    fn add_dedup_of_immediate_repeat() {
        let mut h = History_new(None);
        History_add(&mut h, "same");
        History_add(&mut h, "same");
        // The prior identical entry is removed, then re-appended: no dup.
        assert_eq!(h.entries, vec!["same".to_string()]);
        assert_eq!(h.count, 1);
    }

    #[test]
    fn add_rotates_dropping_oldest_at_cap() {
        let mut h = History_new(None);
        // Add more than the cap of unique entries.
        let total = HISTORY_MAX_ENTRIES + 88; // 600
        for i in 0..total {
            History_add(&mut h, &format!("e{}", i));
        }
        assert_eq!(h.count, HISTORY_MAX_ENTRIES);
        assert_eq!(h.entries.len(), HISTORY_MAX_ENTRIES);
        // Oldest surviving = e{total - cap}; newest = e{total - 1}.
        let oldest = total - HISTORY_MAX_ENTRIES; // 88
        assert_eq!(h.entries.first().unwrap(), &format!("e{}", oldest));
        assert_eq!(h.entries.last().unwrap(), &format!("e{}", total - 1));
        assert_eq!(h.position, HISTORY_MAX_ENTRIES);
    }

    #[test]
    fn capacity_doubles_up_to_cap() {
        let mut h = History_new(None);
        assert_eq!(h.capacity, 64);
        for i in 0..65 {
            History_add(&mut h, &format!("e{}", i));
        }
        // Crossing 64 doubled capacity to 128.
        assert_eq!(h.capacity, 128);
        for i in 65..300 {
            History_add(&mut h, &format!("e{}", i));
        }
        // 64 -> 128 -> 256 -> 512, then it stops at the cap.
        assert_eq!(h.capacity, HISTORY_MAX_ENTRIES);
    }

    #[test]
    fn reset_position_parks_and_clears() {
        let mut h = History_new(None);
        History_add(&mut h, "a");
        History_add(&mut h, "b");
        h.position = 0;
        h.saved.push_str("browsing");
        History_resetPosition(&mut h);
        assert_eq!(h.position, h.count);
        assert_eq!(h.position, 2);
        assert!(h.saved.is_empty());
    }

    #[test]
    fn load_missing_file_is_noop() {
        let path = temp_path("missing");
        let _ = fs::remove_file(&path);
        let h = History_new(Some(path.to_str().unwrap()));
        assert_eq!(h.count, 0);
        assert!(h.entries.is_empty());
    }

    #[test]
    fn load_strips_newlines_skips_blanks_and_dedups() {
        let path = temp_path("load");
        // Blank line skipped, \r\n stripped, duplicate "a" deduped
        // (History_add removes the earlier "a" and re-appends it).
        fs::write(&path, "a\n\nb\r\na\n").unwrap();
        let h = History_new(Some(path.to_str().unwrap()));
        assert_eq!(h.entries, vec!["b".to_string(), "a".to_string()]);
        assert_eq!(h.count, 2);
        // position parked at count by History_new.
        assert_eq!(h.position, 2);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn save_writes_one_entry_per_line() {
        let path = temp_path("save");
        let mut h = History_new(Some(path.to_str().unwrap()));
        History_add(&mut h, "first");
        History_add(&mut h, "second");
        History_save(&h);
        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content, "first\nsecond\n");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn save_then_load_roundtrip() {
        let save_path = temp_path("rt_save");
        let mut h = History_new(Some(save_path.to_str().unwrap()));
        History_add(&mut h, "alpha");
        History_add(&mut h, "beta");
        History_add(&mut h, "gamma");
        History_save(&h);

        let reloaded = History_new(Some(save_path.to_str().unwrap()));
        assert_eq!(
            reloaded.entries,
            vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()]
        );
        assert_eq!(reloaded.count, 3);
        let _ = fs::remove_file(&save_path);
    }

    #[test]
    fn navigate_empty_ring_returns_none() {
        use crate::ported::lineeditor::{LineEditor, LineEditor_init};
        let mut h = History_new(None);
        let mut e = LineEditor::default();
        LineEditor_init(&mut e);
        assert_eq!(History_navigate(&mut h, &e, true), None);
        assert_eq!(History_navigate(&mut h, &e, false), None);
    }

    #[test]
    fn navigate_back_and_forward_walks_and_restores_saved() {
        use crate::ported::lineeditor::{LineEditor, LineEditor_init, LineEditor_setText};
        let mut h = History_new(None);
        History_add(&mut h, "one");
        History_add(&mut h, "two");
        History_add(&mut h, "three");
        // position parked at count (== 3).
        assert_eq!(h.position, 3);

        let mut e = LineEditor::default();
        LineEditor_init(&mut e);
        LineEditor_setText(&mut e, "in-progress");

        // Up arrow: saves editor text, walks toward oldest.
        assert_eq!(History_navigate(&mut h, &e, true), Some("three"));
        assert_eq!(h.saved, "in-progress");
        assert_eq!(History_navigate(&mut h, &e, true), Some("two"));
        assert_eq!(History_navigate(&mut h, &e, true), Some("one"));
        // Already at oldest.
        assert_eq!(History_navigate(&mut h, &e, true), None);
        assert_eq!(h.position, 0);

        // Down arrow: walks back toward newest.
        assert_eq!(History_navigate(&mut h, &e, false), Some("two"));
        assert_eq!(History_navigate(&mut h, &e, false), Some("three"));
        // Returning to "new input" restores the saved text.
        assert_eq!(History_navigate(&mut h, &e, false), Some("in-progress"));
        assert_eq!(h.position, 3);
        // Already at newest.
        assert_eq!(History_navigate(&mut h, &e, false), None);
    }

    #[test]
    fn navigate_saved_only_captured_on_first_step_back() {
        use crate::ported::lineeditor::{LineEditor, LineEditor_init, LineEditor_setText};
        let mut h = History_new(None);
        History_add(&mut h, "a");
        History_add(&mut h, "b");

        let mut e = LineEditor::default();
        LineEditor_init(&mut e);
        LineEditor_setText(&mut e, "first");

        assert_eq!(History_navigate(&mut h, &e, true), Some("b"));
        assert_eq!(h.saved, "first");

        // Change editor text; a deeper step back must NOT overwrite saved,
        // because position != count now.
        LineEditor_setText(&mut e, "changed");
        assert_eq!(History_navigate(&mut h, &e, true), Some("a"));
        assert_eq!(h.saved, "first");
    }

    #[test]
    fn save_none_filename_is_noop() {
        // Constructing with no filename and saving must not panic.
        let mut h = History_new(None);
        History_add(&mut h, "x");
        History_save(&h); // no filename -> returns immediately
        assert_eq!(h.count, 1);
    }
}
