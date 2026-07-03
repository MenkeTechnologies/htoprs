//! Live wiring for the htoprs-original monitoring extensions.
//!
//! This is to the monitoring modules what [`super::overlay`] is to the theme
//! system: a single thread-local state object the running TUI drives through
//! [`ingest`] (fed the real process table each refresh), [`dispatch_key`] (first
//! refusal on keys), and [`draw_active`] (modal chrome over the panels). It owns
//! the persistent engines — [`ProcRing`], [`AlertEngine`], [`FilterStore`],
//! [`Scalar`], and the last [`Snapshot`] — and feeds them the bridged
//! [`Proc`](super::model::Proc) rows from [`super::bridge`].
//!
//! Two of the seven capabilities reach into the process rows themselves rather
//! than a modal: [`alert_attr`] recolors a firing PID's row and [`draw_spark_col`]
//! overdraws its CPU sparkline column. Both are called from the ported
//! `Panel_draw` at the exact per-row draw site (the same extension-hook pattern
//! the theme border uses), so they need no fragile external geometry.
//!
//! Hotkeys (consumed before htop's own key map, only when no theme overlay is
//! open; while a modal is open every key routes here):
//! `f` finder · `r` filter · `d` snapshot/diff · `o` export · `A` alerts ·
//! `G` graph · `v` toggle the sparkline column.

use std::cell::RefCell;
use std::collections::HashSet;
use std::io::Write;

use crossterm::event::KeyCode;
use crossterm::terminal;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use crate::extensions::alerts::{AlertEngine, Firing, Metric, Rule};
use crate::extensions::filter::{Compiled, Field, Filter, FilterStore};
use crate::extensions::graph::Scalar;
use crate::extensions::model::Proc;
use crate::extensions::overlay::{
    blit, draw_box, modal_palette, ncurses_to_keycode, set_str, ModalPalette,
};
use crate::extensions::procring::ProcRing;
use crate::extensions::snapshot::{diff, Diff, Snapshot};
use crate::extensions::{export, finder};
use crate::ported::crt::{ColorElements, ColorScheme};
use crate::ported::functionbar::Ncurses;
use crate::ported::table::Table;

/// Samples kept per PID in the ring / graph history.
const HISTORY: usize = 300;
/// Width in cells of the overdrawn sparkline column.
const SPARK_W: usize = 12;
/// Rows of matches a list modal shows at once.
const LIST_ROWS: usize = 16;

/// Which modal (if any) is currently visible.
#[derive(Clone, Copy, PartialEq)]
enum Modal {
    None,
    Finder,
    Filter,
    Diff,
    Export,
    Alerts,
    Graph,
}

/// The complete live state for the monitoring extensions.
struct PanelState {
    // ── persistent engines ────────────────────────────────────────────────
    ring: ProcRing,
    alerts: AlertEngine,
    filters: FilterStore,
    cpu_hist: Scalar,
    baseline: Option<Snapshot>,

    // ── latest sampled frame ──────────────────────────────────────────────
    table: Vec<Proc>,
    firing: HashSet<u32>,
    firings: Vec<Firing>,
    selected_pid: Option<u32>,
    cpu_peak: f64,
    tick: u64,

    // ── UI ────────────────────────────────────────────────────────────────
    modal: Modal,
    spark_col: bool,
    pending_select: Option<u32>,

    // finder
    finder_query: String,
    finder_hits: Vec<finder::Match>,
    finder_sel: usize,

    // filter
    filter_query: String,
    filter_field: Field,
    filter_regex: bool,
    filter_msg: String,

    // snapshot diff
    diff: Option<Diff>,

    // export
    export_msg: String,
}

impl PanelState {
    fn new() -> Self {
        PanelState {
            ring: ProcRing::new(HISTORY),
            alerts: AlertEngine::new(default_rules()),
            filters: load_filters(),
            cpu_hist: Scalar::new(HISTORY),
            baseline: None,
            table: Vec::new(),
            firing: HashSet::new(),
            firings: Vec::new(),
            selected_pid: None,
            cpu_peak: 100.0,
            tick: 0,
            modal: Modal::None,
            spark_col: false,
            pending_select: None,
            finder_query: String::new(),
            finder_hits: Vec::new(),
            finder_sel: 0,
            filter_query: String::new(),
            filter_field: Field::Any,
            filter_regex: false,
            filter_msg: String::new(),
            diff: None,
            export_msg: String::new(),
        }
    }

    /// One real refresh: advance every engine on the freshly-bridged table.
    fn ingest(&mut self, rows: Vec<Proc>, selected: Option<u32>) {
        self.ring.record(&rows);
        let total: f64 = rows.iter().map(|p| p.cpu as f64).sum();
        self.cpu_hist.push(total);
        if total > self.cpu_peak {
            self.cpu_peak = total;
        }
        self.firings = self.alerts.evaluate(&rows);
        self.firing = self.firings.iter().map(|f| f.pid).collect();
        self.selected_pid = selected;
        self.table = rows;
        self.tick += 1;
        if self.modal == Modal::Finder {
            self.recompute_finder();
        }
    }

    fn any_active(&self) -> bool {
        self.modal != Modal::None
    }

    // ── candidate lists ───────────────────────────────────────────────────

    /// "comm  cmdline" candidate for each table row, in table order.
    fn candidates(&self) -> Vec<String> {
        self.table
            .iter()
            .map(|p| format!("{} {}", p.comm, p.cmdline))
            .collect()
    }

    fn recompute_finder(&mut self) {
        let cands = self.candidates();
        self.finder_hits = finder::fuzzy(&self.finder_query, &cands);
        if self.finder_sel >= self.finder_hits.len() {
            self.finder_sel = self.finder_hits.len().saturating_sub(1);
        }
    }

    fn compiled_filter(&self) -> Option<Compiled> {
        Filter {
            name: self.filter_query.clone(),
            pattern: self.filter_query.clone(),
            regex: self.filter_regex,
            field: self.filter_field,
        }
        .compile()
        .ok()
    }

    // ── key handling ──────────────────────────────────────────────────────

    /// Returns `true` if this key was consumed.
    fn handle(&mut self, ch: i32) -> bool {
        // While a modal is open every key routes here (mirrors the overlay).
        if self.modal != Modal::None {
            // Tab (9) is outside the printable range `ncurses_to_keycode`
            // maps, but the filter modal uses it to cycle fields.
            let code = match ch {
                9 => Some(KeyCode::Char('\t')),
                _ => ncurses_to_keycode(ch),
            };
            self.handle_modal(code);
            return true;
        }
        // Idle: yield to the theme overlay if it owns the frame.
        if crate::extensions::overlay::overlay_active() {
            return false;
        }
        // Idle hotkeys. Bytes chosen from keys htop leaves unbound.
        match ch {
            0x66 => self.open_finder(),         // 'f'
            0x72 => self.open_filter(),         // 'r'
            0x64 => self.snapshot_action(),     // 'd'
            0x6f => self.export_action(),       // 'o'
            0x41 => self.modal = Modal::Alerts, // 'A'
            0x47 => self.modal = Modal::Graph,  // 'G'
            0x76 => self.toggle_spark(),        // 'v'
            _ => return false,
        }
        true
    }

    fn handle_modal(&mut self, code: Option<KeyCode>) {
        let Some(code) = code else { return };
        if code == KeyCode::Esc {
            self.modal = Modal::None;
            return;
        }
        match self.modal {
            Modal::Finder => self.finder_key(code),
            Modal::Filter => self.filter_key(code),
            Modal::Diff => self.diff_key(code),
            Modal::Export | Modal::Alerts | Modal::Graph => {
                // Info-only modals: any non-Esc key closes.
                self.modal = Modal::None;
            }
            Modal::None => {}
        }
    }

    fn open_finder(&mut self) {
        self.modal = Modal::Finder;
        self.finder_query.clear();
        self.finder_sel = 0;
        self.recompute_finder();
    }

    fn finder_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char(c) => {
                self.finder_query.push(c);
                self.finder_sel = 0;
                self.recompute_finder();
            }
            KeyCode::Backspace => {
                self.finder_query.pop();
                self.finder_sel = 0;
                self.recompute_finder();
            }
            KeyCode::Down => {
                if self.finder_sel + 1 < self.finder_hits.len() {
                    self.finder_sel += 1;
                }
            }
            KeyCode::Up => self.finder_sel = self.finder_sel.saturating_sub(1),
            KeyCode::Enter => {
                if let Some(m) = self.finder_hits.get(self.finder_sel) {
                    if let Some(p) = self.table.get(m.idx) {
                        self.pending_select = Some(p.pid);
                    }
                }
                self.modal = Modal::None;
            }
            _ => {}
        }
    }

    fn open_filter(&mut self) {
        self.modal = Modal::Filter;
        self.filter_msg.clear();
    }

    fn filter_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char('\t') => {
                self.filter_field = match self.filter_field {
                    Field::Any => Field::Comm,
                    Field::Comm => Field::Cmdline,
                    Field::Cmdline => Field::User,
                    Field::User => Field::Any,
                };
            }
            KeyCode::Char('~') => self.filter_regex = !self.filter_regex,
            KeyCode::Char(c) => self.filter_query.push(c),
            KeyCode::Backspace => {
                self.filter_query.pop();
            }
            KeyCode::Enter => self.save_filter(),
            _ => {}
        }
    }

    fn save_filter(&mut self) {
        if self.filter_query.is_empty() {
            self.filter_msg = "empty pattern — nothing saved".into();
            return;
        }
        match self.compiled_filter() {
            Some(_) => {
                self.filters.put(Filter {
                    name: self.filter_query.clone(),
                    pattern: self.filter_query.clone(),
                    regex: self.filter_regex,
                    field: self.filter_field,
                });
                save_filters(&self.filters);
                self.filter_msg = format!("saved \"{}\"", self.filter_query);
            }
            None => self.filter_msg = "invalid regex — not saved".into(),
        }
    }

    fn snapshot_action(&mut self) {
        match &self.baseline {
            None => {
                self.baseline = Some(Snapshot::capture(self.tick, &self.table));
                self.diff = None;
            }
            Some(base) => {
                let now = Snapshot::capture(self.tick, &self.table);
                self.diff = Some(diff(base, &now));
            }
        }
        self.modal = Modal::Diff;
    }

    fn diff_key(&mut self, code: KeyCode) {
        match code {
            // 'r' resets the baseline to the current table.
            KeyCode::Char('r') => {
                self.baseline = Some(Snapshot::capture(self.tick, &self.table));
                self.diff = None;
            }
            // 'w' writes the current baseline snapshot to the config dir.
            KeyCode::Char('w') => {
                if let Some(base) = &self.baseline {
                    let name = format!("snapshot-{}.json", base.tick);
                    match write_artifact(&name, &base.to_json()) {
                        Some(path) => self.export_msg = format!("wrote {path}"),
                        None => self.export_msg = "write failed (no config dir)".into(),
                    }
                }
            }
            _ => self.modal = Modal::None,
        }
    }

    fn export_action(&mut self) {
        let json = export::to_json(&self.table);
        let csv = export::to_csv(&self.table);
        let jn = format!("export-{}.json", self.tick);
        let cn = format!("export-{}.csv", self.tick);
        let jp = write_artifact(&jn, &json);
        let cp = write_artifact(&cn, &csv);
        self.export_msg = match (jp, cp) {
            (Some(a), Some(b)) => format!("{a}\n{b}"),
            _ => "export failed (no config dir)".into(),
        };
        self.modal = Modal::Export;
    }

    fn toggle_spark(&mut self) {
        self.spark_col = !self.spark_col;
    }

    // ── rendering ─────────────────────────────────────────────────────────

    fn render(&self, buf: &mut Buffer, area: Rect) {
        let s = Sty::new();
        match self.modal {
            Modal::Finder => self.render_finder(buf, area, &s),
            Modal::Filter => self.render_filter(buf, area, &s),
            Modal::Diff => self.render_diff(buf, area, &s),
            Modal::Export => self.render_lines(
                buf,
                area,
                &s,
                "Export — current table written",
                self.export_msg
                    .lines()
                    .map(|l| (l.to_string(), s.body))
                    .collect(),
            ),
            Modal::Alerts => self.render_alerts(buf, area, &s),
            Modal::Graph => self.render_graph(buf, area, &s),
            Modal::None => {}
        }
    }

    /// Draw a centered box titled `title` holding `lines` of pre-styled text.
    fn render_lines(
        &self,
        buf: &mut Buffer,
        area: Rect,
        s: &Sty,
        title: &str,
        lines: Vec<(String, Style)>,
    ) {
        let inner_w = lines
            .iter()
            .map(|(t, _)| t.chars().count())
            .chain(std::iter::once(title.chars().count()))
            .max()
            .unwrap_or(20)
            .clamp(24, area.width.saturating_sub(4).max(24) as usize);
        let bw = (inner_w as u16 + 4).min(area.width);
        let bh = (lines.len() as u16 + 4).min(area.height);
        let (x0, y0) = draw_box(buf, area, bw, bh, s.bg, s.border);
        set_str(buf, x0 + 2, y0, &format!(" {title} "), s.title, bw - 3);
        for (i, (t, st)) in lines.iter().enumerate() {
            if i as u16 + 1 >= bh - 1 {
                break;
            }
            set_str(buf, x0 + 2, y0 + 2 + i as u16, t, *st, bw - 3);
        }
    }

    fn render_finder(&self, buf: &mut Buffer, area: Rect, s: &Sty) {
        let mut lines = Vec::new();
        lines.push((
            format!("> {}▏", self.finder_query),
            s.body.add_modifier(Modifier::BOLD),
        ));
        lines.push((
            format!(
                "{} matches · ↑/↓ move · Enter jump · Esc cancel",
                self.finder_hits.len()
            ),
            s.dim,
        ));
        for (row, m) in self.finder_hits.iter().take(LIST_ROWS).enumerate() {
            let Some(p) = self.table.get(m.idx) else {
                continue;
            };
            let line = format!(
                "{:>7}  {:<14} {}",
                p.pid,
                trunc(&p.comm, 14),
                trunc(&p.cmdline, 48)
            );
            let st = if row == self.finder_sel {
                s.sel
            } else {
                s.body
            };
            lines.push((line, st));
        }
        self.render_lines(buf, area, s, "Fuzzy process finder", lines);
    }

    fn render_filter(&self, buf: &mut Buffer, area: Rect, s: &Sty) {
        let field = match self.filter_field {
            Field::Any => "any",
            Field::Comm => "comm",
            Field::Cmdline => "cmdline",
            Field::User => "user",
        };
        let mode = if self.filter_regex {
            "regex"
        } else {
            "substring"
        };
        let mut lines = vec![
            (
                format!("/ {}▏", self.filter_query),
                s.body.add_modifier(Modifier::BOLD),
            ),
            (
                format!(
                    "field: {field}  mode: {mode}  · Tab field · ~ regex · Enter save · Esc close"
                ),
                s.dim,
            ),
        ];
        match self.compiled_filter() {
            Some(c) => {
                let hits: Vec<&Proc> = self.table.iter().filter(|p| c.matches(p)).collect();
                lines.push((format!("{} live matches", hits.len()), s.body));
                for p in hits.iter().take(LIST_ROWS - 2) {
                    lines.push((
                        format!(
                            "{:>7}  {:<14} {}",
                            p.pid,
                            trunc(&p.comm, 14),
                            trunc(&p.cmdline, 46)
                        ),
                        s.body,
                    ));
                }
            }
            None if !self.filter_query.is_empty() => {
                lines.push(("invalid regex".to_string(), s.alert))
            }
            None => {}
        }
        if !self.filter_msg.is_empty() {
            lines.push((self.filter_msg.clone(), s.title));
        }
        if !self.filters.filters.is_empty() {
            let names: Vec<&str> = self
                .filters
                .filters
                .iter()
                .map(|f| f.name.as_str())
                .collect();
            lines.push((format!("saved: {}", names.join(", ")), s.dim));
        }
        self.render_lines(buf, area, s, "Regex / saved filters", lines);
    }

    fn render_diff(&self, buf: &mut Buffer, area: Rect, s: &Sty) {
        let mut lines = Vec::new();
        match &self.diff {
            None => {
                let n = self.baseline.as_ref().map(|b| b.procs.len()).unwrap_or(0);
                lines.push((format!("baseline captured — {n} processes"), s.body));
                lines.push(("press d again to diff against it".into(), s.dim));
            }
            Some(d) => {
                lines.push((
                    format!(
                        "+{} started  -{} exited  ~{} changed",
                        d.added.len(),
                        d.removed.len(),
                        d.changed.len()
                    ),
                    s.body.add_modifier(Modifier::BOLD),
                ));
                for p in d.added.iter().take(5) {
                    lines.push((format!("+ {:>7} {}", p.pid, trunc(&p.comm, 40)), s.started));
                }
                for p in d.removed.iter().take(5) {
                    lines.push((format!("- {:>7} {}", p.pid, trunc(&p.comm, 40)), s.alert));
                }
                for c in d.changed.iter().take(6) {
                    lines.push((
                        format!(
                            "~ {:>7} {} cpu {:.0}→{:.0}",
                            c.pid,
                            trunc(&c.after.comm, 20),
                            c.before.cpu,
                            c.after.cpu
                        ),
                        s.body,
                    ));
                }
            }
        }
        lines.push(("r reset baseline · w write json · Esc close".into(), s.dim));
        self.render_lines(buf, area, s, "Snapshot diff", lines);
    }

    fn render_alerts(&self, buf: &mut Buffer, area: Rect, s: &Sty) {
        let mut lines = vec![("rules:".to_string(), s.dim)];
        for r in self.alerts_rules_view() {
            lines.push((r, s.body));
        }
        lines.push((format!("firing now: {}", self.firings.len()), s.title));
        for f in self.firings.iter().take(LIST_ROWS - 4) {
            lines.push((
                format!(
                    "! {:>7}  {}  = {:.0}  ({} ticks)",
                    f.pid, f.rule, f.value, f.sustained
                ),
                s.alert,
            ));
        }
        lines.push(("Esc close".into(), s.dim));
        self.render_lines(buf, area, s, "Threshold alerts", lines);
    }

    fn alerts_rules_view(&self) -> Vec<String> {
        default_rules()
            .iter()
            .map(|r| {
                let m = match r.metric {
                    Metric::Cpu => "cpu%",
                    Metric::MemKb => "mem_kb",
                };
                format!(
                    "  {} : {} ≥ {:.0} for {} ticks",
                    r.name, m, r.threshold, r.for_ticks
                )
            })
            .collect()
    }

    fn render_graph(&self, buf: &mut Buffer, area: Rect, s: &Sty) {
        let width = 48usize;
        let height = 6usize;
        let max = self.cpu_peak.max(1.0);
        let rows = self.cpu_hist.render(width, height, max);
        let mut lines: Vec<(String, Style)> = vec![(
            format!(
                "total CPU — peak {max:.0}%  ({} samples)",
                self.cpu_hist.len()
            ),
            s.dim,
        )];
        for r in rows {
            lines.push((r, s.spark));
        }
        match self.selected_pid {
            Some(pid) => {
                let spark = self.ring.cpu_sparkline(pid, width, 100.0);
                lines.push((format!("pid {pid}: {spark}"), s.spark));
            }
            None => lines.push(("(select a process for its CPU history)".into(), s.dim)),
        }
        lines.push(("Esc close".into(), s.dim));
        self.render_lines(buf, area, s, "CPU history graph", lines);
    }
}

// ─── default rules ──────────────────────────────────────────────────────────

fn default_rules() -> Vec<Rule> {
    vec![
        Rule {
            name: "hot-cpu".into(),
            metric: Metric::Cpu,
            threshold: 90.0,
            for_ticks: 3,
        },
        Rule {
            name: "big-mem".into(),
            metric: Metric::MemKb,
            threshold: 2_000_000.0,
            for_ticks: 3,
        },
    ]
}

// ─── persistence ────────────────────────────────────────────────────────────

fn filters_path() -> Option<std::path::PathBuf> {
    crate::extensions::prefs::config_dir().map(|d| d.join("filters.json"))
}

fn load_filters() -> FilterStore {
    filters_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| FilterStore::from_json(&s).ok())
        .unwrap_or_default()
}

fn save_filters(store: &FilterStore) {
    if let Some(p) = filters_path() {
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(p, store.to_json());
    }
}

/// Write `contents` to `<config dir>/<name>`, returning the display path.
fn write_artifact(name: &str, contents: &str) -> Option<String> {
    let dir = crate::extensions::prefs::config_dir()?;
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(name);
    std::fs::write(&path, contents).ok()?;
    Some(path.display().to_string())
}

// ─── styles (resolved from the active theme, so modals track the colorscheme) ─

/// The modal's styles for one frame, built from the live theme palette. The
/// alert/started colors stay semantic (red = over-threshold, green = started)
/// but on the themed background so they blend with the chosen colorscheme.
struct Sty {
    bg: Color,
    border: Style,
    title: Style,
    body: Style,
    dim: Style,
    sel: Style,
    alert: Style,
    started: Style,
    spark: Style,
}

impl Sty {
    fn new() -> Self {
        let p: ModalPalette = modal_palette();
        Sty {
            bg: p.bg,
            border: Style::default().fg(p.border),
            title: Style::default()
                .fg(p.title)
                .bg(p.bg)
                .add_modifier(Modifier::BOLD),
            body: Style::default().fg(p.text).bg(p.bg),
            dim: Style::default().fg(Color::Indexed(240)).bg(p.bg),
            sel: Style::default().fg(p.bg).bg(p.accent),
            alert: Style::default()
                .fg(Color::Red)
                .bg(p.bg)
                .add_modifier(Modifier::BOLD),
            started: Style::default().fg(Color::Green).bg(p.bg),
            spark: Style::default().fg(p.accent).bg(p.bg),
        }
    }
}

fn trunc(s: &str, w: usize) -> String {
    if s.chars().count() <= w {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(w.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

// ─── thread-local live state + public API ───────────────────────────────────

thread_local! {
    /// Live monitoring state for the running TUI. Thread-local for the same
    /// reason as [`super::overlay`]: `ScreenManager_run` draws and reads keys
    /// on one thread.
    static PANELS: RefCell<PanelState> = RefCell::new(PanelState::new());
}

/// Feed one real refresh: bridge the live table and advance every engine.
/// Called from the run loop on each sample tick (not on key redraws), so the
/// history rings advance once per refresh like `ProcRing::record` expects.
pub fn ingest(table: &Table) {
    let rows = crate::extensions::bridge::snapshot_table(table);
    let selected = crate::extensions::bridge::selected_pid(table);
    PANELS.with(|p| p.borrow_mut().ingest(rows, selected));
}

/// Route a raw ncurses key. Returns `true` if consumed (a hotkey when idle, or
/// any key while a modal is open). Yields to the theme overlay when it is open.
pub fn dispatch_key(ch: i32) -> bool {
    PANELS.with(|p| p.borrow_mut().handle(ch))
}

/// Whether a monitoring modal is currently visible (keeps the run loop
/// repainting it, like [`super::overlay::overlay_active`]).
pub fn panel_active() -> bool {
    PANELS.with(|p| p.borrow().any_active())
}

/// A pid the finder asked to jump to, consumed once. The run loop applies it
/// with `Panel_setSelected` after dispatch.
pub fn take_pending_select() -> Option<u32> {
    PANELS.with(|p| p.borrow_mut().pending_select.take())
}

/// Draw the active modal (if any) over the current screen, then flush.
pub fn draw_active<W: Write>(out: &mut W) {
    let (cols, rows) = terminal::size().unwrap_or((80, 24));
    // Below this the modal box math (fixed 24-col inner width) has no room;
    // skip rather than risk a sub-zero width.
    if cols < 30 || rows < 8 {
        return;
    }
    PANELS.with(|p| {
        let s = p.borrow();
        if !s.any_active() {
            return;
        }
        let area = Rect::new(0, 0, cols, rows);
        let mut b = Buffer::empty(area);
        s.render(&mut b, area);
        blit(out, &b);
    });
    let _ = out.flush();
}

/// The row color for a firing PID, consulted by `Panel_draw` before it paints a
/// process row. `None` for non-firing PIDs (the row keeps its normal color).
pub fn alert_attr(pid: u32) -> Option<i32> {
    PANELS.with(|p| {
        let s = p.borrow();
        if s.firing.contains(&pid) {
            Some(ColorElements::FAILED_SEARCH.packed(ColorScheme::active()))
        } else {
            None
        }
    })
}

/// Overdraw the CPU sparkline column at the right edge of a process row, when
/// the `v` toggle is on. Called by `Panel_draw` after it prints the row, with
/// the row's htop-space `(y, x)` and panel width `w` (the `Ncurses` shim adds
/// the border margin). A no-op when the column is off or the PID has no history.
pub fn draw_spark_col<W: Write>(out: &mut W, y: i32, x: i32, w: i32, pid: u32) {
    PANELS.with(|p| {
        let s = p.borrow();
        if !s.spark_col || w as usize <= SPARK_W + 2 {
            return;
        }
        // `cpu_sparkline` already returns exactly `SPARK_W` glyphs, so print the
        // whole string. `mvaddnstr`'s `n` is a *byte* limit and the braille
        // glyphs are 3 bytes each — an `n`-slice lands mid-char and panics.
        let spark = s.ring.cpu_sparkline(pid, SPARK_W, 100.0);
        let sx = x + w - SPARK_W as i32;
        Ncurses::attrset(
            out,
            ColorElements::PROCESS_MEGABYTES.packed(ColorScheme::active()),
        );
        Ncurses::mvaddstr(out, y, sx, &spark);
        Ncurses::attrset(
            out,
            ColorElements::RESET_COLOR.packed(ColorScheme::active()),
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extensions::model::synthetic_table;

    /// A fresh state on this test thread (thread-local isolation).
    fn ingest_ticks(n: u64) {
        for t in 0..n {
            let rows = synthetic_table(t);
            PANELS.with(|p| p.borrow_mut().ingest(rows, Some(200)));
        }
    }

    #[test]
    fn hotkey_opens_and_esc_closes_finder() {
        assert!(!panel_active());
        assert!(dispatch_key(0x66)); // 'f'
        assert!(panel_active());
        assert!(dispatch_key(27)); // Esc
        assert!(!panel_active());
    }

    #[test]
    fn idle_non_hotkey_not_consumed() {
        assert!(!dispatch_key(0x6b)); // 'k' — htop's kill, not ours
    }

    #[test]
    fn finder_filters_by_query() {
        ingest_ticks(1);
        dispatch_key(0x66); // 'f'
        for b in b"firefox" {
            dispatch_key(*b as i32);
        }
        let hits = PANELS.with(|p| p.borrow().finder_hits.len());
        assert!(hits >= 1, "expected firefox rows, got {hits}");
        dispatch_key(27);
    }

    #[test]
    fn snapshot_then_diff_populates() {
        ingest_ticks(1); // baseline table has pid 500
        dispatch_key(0x64); // 'd' -> capture baseline, opens Diff modal
        assert!(panel_active());
        dispatch_key(27);
        // advance so pid 500 disappears, then diff
        PANELS.with(|p| p.borrow_mut().ingest(synthetic_table(4), None));
        dispatch_key(0x64); // 'd' -> diff
        let removed = PANELS.with(|p| {
            p.borrow()
                .diff
                .as_ref()
                .map(|d| d.removed.len())
                .unwrap_or(0)
        });
        assert!(removed >= 1, "pid 500 should show as removed");
        dispatch_key(27);
    }

    #[test]
    fn spark_toggle_and_alert_attr() {
        // 'v' toggles the column flag.
        dispatch_key(0x76);
        assert!(PANELS.with(|p| p.borrow().spark_col));
        dispatch_key(0x76);
        assert!(!PANELS.with(|p| p.borrow().spark_col));
        // A sustained 90%+ pid fires after for_ticks and yields a recolor.
        let hot = Proc {
            pid: 999,
            ppid: 1,
            user: "u".into(),
            comm: "hot".into(),
            cmdline: "hot".into(),
            state: 'R',
            cpu: 99.0,
            mem_kb: 1,
        };
        for _ in 0..3 {
            PANELS.with(|p| p.borrow_mut().ingest(vec![hot.clone()], None));
        }
        assert!(alert_attr(999).is_some());
        assert!(alert_attr(1).is_none());
    }

    #[test]
    fn spark_col_never_panics_on_braille() {
        // Regression: the braille sparkline is 3-byte glyphs; a byte-length
        // `mvaddnstr` slice landed mid-char and panicked. Enable the column,
        // build up history, and render into a sink — must not panic.
        dispatch_key(0x76); // 'v' on
        for t in 0..40 {
            PANELS.with(|p| p.borrow_mut().ingest(synthetic_table(t), Some(200)));
        }
        let mut sink: Vec<u8> = Vec::new();
        // pid 200 has ~40 samples -> a full-width multi-byte sparkline.
        draw_spark_col(&mut sink, 3, 0, 80, 200);
        assert!(!sink.is_empty());
        dispatch_key(0x76); // 'v' off (leave state clean)
    }
}
