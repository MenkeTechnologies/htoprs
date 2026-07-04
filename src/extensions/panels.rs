//! Live wiring for the htoprs-original monitoring extensions.
//!
//! This is to the monitoring modules what [`super::overlay`] is to the theme
//! system: a single thread-local state object the running TUI drives through
//! [`ingest`] (fed the real process table each refresh), [`dispatch_key`] (first
//! refusal on keys), and [`draw_active`] (modal chrome over the panels). It owns
//! the persistent engines — [`ProcRing`], [`AlertEngine`], [`FilterStore`],
//! [`Scalar`], and the last [`Snapshot`] — and feeds them the bridged
//! [`Proc`] rows from [`super::bridge`].
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
use serde::{Deserialize, Serialize};

use crate::extensions::aggregate::{aggregate, GroupBy};
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
/// Physical terminal lines the CPU graph occupies beneath each process in
/// double-height mode. Each line is 4 braille dot-rows, so the graph shows
/// `SPARK_GRAPH_H * 4` levels of amplitude. Tune here; the process panel's
/// `rowHeight` becomes `1 + SPARK_GRAPH_H` (the info line plus the graph).
const SPARK_GRAPH_H: i32 = 3;
/// Rows of matches a list modal shows at once.
const LIST_ROWS: usize = 16;

/// Show a transient confirmation toast for a monitoring-hotkey action, reusing
/// the same status-toast channel the theme overlay and the `b` bar-style cycler
/// use (`overlay::draw_status`, auto-dismissed after 3s).
fn toast(msg: impl Into<String>) {
    crate::extensions::overlay::set_status(msg);
}

/// How the per-PID CPU sparkline is shown, cycled by `v`. Persisted to prefs.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum SparkMode {
    /// No sparkline.
    #[default]
    Off,
    /// A narrow `SPARK_W`-cell sparkline overdrawn at each row's right edge,
    /// keeping the 1-item-1-line layout.
    Column,
    /// Each process occupies two physical lines: the normal row plus a
    /// full-width CPU sparkline beneath it (the panel's `rowHeight` is 2).
    Double,
}

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
    Aggregate,
    Palette,
}

/// The command-palette action registry (`:`): a searchable name paired with the
/// key it injects into the run loop. Injection flows through the whole dispatch
/// pipeline, so both htoprs-extension actions and htop's own keys are reachable.
const CMDS: &[(&str, i32)] = &[
    ("finder — fuzzy process search", b'f' as i32),
    ("filter — regex / saved filters", b'r' as i32),
    ("snapshot / diff process table", b'd' as i32),
    ("export table to JSON / CSV", b'o' as i32),
    ("alerts — threshold rules", b'A' as i32),
    ("cpu history graph", b'G' as i32),
    ("aggregate / pivot totals", b'y' as i32),
    ("sparkline — cycle per-PID CPU graph", b'v' as i32),
    ("bar style — cycle fill glyph", b'b' as i32),
    ("border — toggle", b'B' as i32),
    ("header — toggle", b'g' as i32),
    ("theme — chooser", b'c' as i32),
    ("theme — editor", b'C' as i32),
    ("help", b'h' as i32),
    ("kill process", b'k' as i32),
    ("tree view — toggle", b't' as i32),
    ("search", b'/' as i32),
    ("setup", b'S' as i32),
];

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
    spark: SparkMode,
    /// The key the aggregate/pivot modal (`y`) rolls the table up on. Cycled
    /// with `Tab` and persisted to prefs.
    agg_by: GroupBy,
    /// Whether firing (over-threshold) rows get the hot-row recolor. Toggled
    /// from the Alerts modal (`A`, then `t`); the alert engine keeps evaluating
    /// either way, so the modal's firing counts stay live when this is off.
    alert_hl: bool,
    pending_select: Option<u32>,
    /// A key the command palette (`:`) picked, injected into the run loop's
    /// key read so it flows through the normal dispatch pipeline.
    pending_key: Option<i32>,

    // finder
    finder_query: String,
    finder_hits: Vec<finder::Match>,
    finder_sel: usize,

    // command palette (`:`)
    palette_query: String,
    palette_hits: Vec<finder::Match>,
    palette_sel: usize,

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
        // Restore the persisted extension toggles once (spark mode, hot-row
        // highlight); absent (first run) falls back to each field's default.
        let saved = super::prefs::load();
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
            spark: saved.as_ref().map(|p| p.spark).unwrap_or_default(),
            agg_by: saved.as_ref().map(|p| p.agg_by).unwrap_or_default(),
            // Restore the saved hot-row-highlight toggle; absent (first run) =
            // off (opt-in via the Alerts modal `A` → `t`).
            alert_hl: saved.as_ref().and_then(|p| p.alert_hl).unwrap_or(false),
            pending_select: None,
            pending_key: None,
            finder_query: String::new(),
            finder_hits: Vec::new(),
            finder_sel: 0,
            palette_query: String::new(),
            palette_hits: Vec::new(),
            palette_sel: 0,
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
        // Idle hotkeys. Bytes chosen from keys htop leaves unbound. Each surfaces
        // a transient confirmation toast (`overlay::set_status`), like `b`.
        match ch {
            0x66 => self.open_finder(),     // 'f'
            0x72 => self.open_filter(),     // 'r'
            0x64 => self.snapshot_action(), // 'd'
            0x6f => self.export_action(),   // 'o'
            0x41 => {
                // 'A'
                self.modal = Modal::Alerts;
                toast(format!("Alerts — {} firing", self.firing.len()));
            }
            0x47 => {
                // 'G'
                self.modal = Modal::Graph;
                toast("CPU history graph");
            }
            0x76 => self.toggle_spark(), // 'v'
            0x79 => {
                // 'y' — open the aggregation/pivot rollup.
                self.modal = Modal::Aggregate;
                toast(format!("Aggregate by {}", self.agg_by.label()));
            }
            0x3a => self.open_palette(), // ':' — command palette
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
            Modal::Palette => self.palette_key(code),
            Modal::Filter => self.filter_key(code),
            Modal::Diff => self.diff_key(code),
            Modal::Alerts => {
                // 't' toggles the hot-row highlight in place (and persists it to
                // prefs so it survives restarts); any other non-Esc key closes.
                if code == KeyCode::Char('t') {
                    self.alert_hl = !self.alert_hl;
                    let on = self.alert_hl;
                    super::prefs::update(|p| p.alert_hl = Some(on));
                    toast(if on {
                        "Hot-row highlight: on"
                    } else {
                        "Hot-row highlight: off"
                    });
                } else {
                    self.modal = Modal::None;
                }
            }
            Modal::Aggregate => {
                // Tab cycles the group-by key (and persists it); any other
                // non-Esc key closes. Tab arrives as `Char('\t')` (see `handle`).
                if code == KeyCode::Char('\t') {
                    self.agg_by = self.agg_by.next();
                    let by = self.agg_by;
                    super::prefs::update(|p| p.agg_by = by);
                    toast(format!("Aggregate by {}", by.label()));
                } else {
                    self.modal = Modal::None;
                }
            }
            Modal::Export | Modal::Graph => {
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
        toast("Process finder");
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

    fn open_palette(&mut self) {
        self.modal = Modal::Palette;
        self.palette_query.clear();
        self.palette_sel = 0;
        self.recompute_palette();
        toast("Command palette");
    }

    /// Fuzzy-match the query against the [`CMDS`] action names.
    fn recompute_palette(&mut self) {
        let names: Vec<String> = CMDS.iter().map(|(n, _)| n.to_string()).collect();
        self.palette_hits = finder::fuzzy(&self.palette_query, &names);
        if self.palette_sel >= self.palette_hits.len() {
            self.palette_sel = self.palette_hits.len().saturating_sub(1);
        }
    }

    fn palette_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char(c) => {
                self.palette_query.push(c);
                self.palette_sel = 0;
                self.recompute_palette();
            }
            KeyCode::Backspace => {
                self.palette_query.pop();
                self.palette_sel = 0;
                self.recompute_palette();
            }
            KeyCode::Down => {
                if self.palette_sel + 1 < self.palette_hits.len() {
                    self.palette_sel += 1;
                }
            }
            KeyCode::Up => self.palette_sel = self.palette_sel.saturating_sub(1),
            KeyCode::Enter => {
                // Queue the selected action's key for the run loop to inject.
                if let Some(m) = self.palette_hits.get(self.palette_sel) {
                    if let Some((_, key)) = CMDS.get(m.idx) {
                        self.pending_key = Some(*key);
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
        toast("Filter");
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
                toast("Snapshot baseline captured");
            }
            Some(base) => {
                let now = Snapshot::capture(self.tick, &self.table);
                let d = diff(base, &now);
                toast(format!(
                    "Diff: +{} -{} ~{}",
                    d.added.len(),
                    d.removed.len(),
                    d.changed.len()
                ));
                self.diff = Some(d);
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
                toast("Baseline reset");
            }
            // 'w' writes the current baseline snapshot to the config dir.
            KeyCode::Char('w') => {
                if let Some(base) = &self.baseline {
                    let name = format!("snapshot-{}.json", base.tick);
                    match write_artifact(&name, &base.to_json()) {
                        Some(path) => {
                            self.export_msg = format!("wrote {path}");
                            toast("Snapshot written");
                        }
                        None => {
                            self.export_msg = "write failed (no config dir)".into();
                            toast("Snapshot write failed");
                        }
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
            (Some(a), Some(b)) => {
                toast("Exported JSON + CSV");
                format!("{a}\n{b}")
            }
            _ => {
                toast("Export failed — no config dir");
                "export failed (no config dir)".into()
            }
        };
        self.modal = Modal::Export;
    }

    /// Cycle the sparkline display: Off → side column → double-height → Off.
    fn toggle_spark(&mut self) {
        self.spark = match self.spark {
            SparkMode::Off => SparkMode::Column,
            SparkMode::Column => SparkMode::Double,
            SparkMode::Double => SparkMode::Off,
        };
        toast(match self.spark {
            SparkMode::Off => "CPU graph: off",
            SparkMode::Column => "CPU graph: column",
            SparkMode::Double => "CPU graph: inline (taller = busier)",
        });
        let spark = self.spark;
        super::prefs::update(|p| p.spark = spark);
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
            Modal::Aggregate => self.render_aggregate(buf, area, &s),
            Modal::Palette => self.render_palette(buf, area, &s),
            Modal::None => {}
        }
    }

    /// The command palette (`:`): a fuzzy-searchable list of actions, each
    /// executed by injecting its key into the run loop on Enter.
    fn render_palette(&self, buf: &mut Buffer, area: Rect, s: &Sty) {
        let mut lines = Vec::new();
        lines.push((
            format!("> {}▏", self.palette_query),
            s.body.add_modifier(Modifier::BOLD),
        ));
        lines.push((
            format!(
                "{} commands · ↑/↓ move · Enter run · Esc cancel",
                self.palette_hits.len()
            ),
            s.dim,
        ));
        for (row, m) in self.palette_hits.iter().take(LIST_ROWS).enumerate() {
            let Some((name, _)) = CMDS.get(m.idx) else {
                continue;
            };
            let st = if row == self.palette_sel {
                s.sel
            } else {
                s.body
            };
            lines.push((format!("  {name}"), st));
        }
        self.render_lines(buf, area, s, "Command palette", lines);
    }

    /// The aggregation/pivot modal (`y`): the live table rolled up on the
    /// current [`GroupBy`] key, sorted by CPU, top `LIST_ROWS` groups.
    fn render_aggregate(&self, buf: &mut Buffer, area: Rect, s: &Sty) {
        let groups = aggregate(&self.table, self.agg_by);
        let mut lines = Vec::new();
        lines.push((
            format!(
                "by {} · {} groups · Tab cycle · Esc close",
                self.agg_by.label(),
                groups.len()
            ),
            s.dim,
        ));
        lines.push((
            format!("{:<22} {:>5} {:>7} {:>10}", "KEY", "PROCS", "CPU%", "MEM"),
            s.body.add_modifier(Modifier::BOLD),
        ));
        for g in groups.iter().take(LIST_ROWS) {
            lines.push((
                format!(
                    "{:<22} {:>5} {:>6.1} {:>10}",
                    trunc(&g.key, 22),
                    g.count,
                    g.cpu,
                    human_kb(g.mem_kb)
                ),
                s.body,
            ));
        }
        self.render_lines(buf, area, s, "Aggregate", lines);
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
        let hl = if self.alert_hl { "on" } else { "off" };
        let mut lines = vec![
            (format!("row highlight: {hl}   (t: toggle)"), s.title),
            ("rules:".to_string(), s.dim),
        ];
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
        lines.push(("t toggle highlight · Esc close".into(), s.dim));
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

/// Format a KB count as a human-readable `K`/`M`/`G` string (base-1024), e.g.
/// `512K`, `3.9M`, `2.1G` — for the aggregate modal's memory column.
fn human_kb(kb: u64) -> String {
    const M: u64 = 1024;
    const G: u64 = 1024 * 1024;
    if kb >= G {
        format!("{:.1}G", kb as f64 / G as f64)
    } else if kb >= M {
        format!("{:.1}M", kb as f64 / M as f64)
    } else {
        format!("{kb}K")
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

/// A key the command palette (`:`) picked, consumed once. The run loop injects
/// it in place of the next `Panel_getCh`, so the action runs through the normal
/// dispatch pipeline (extension + htop) as if the user had typed it.
pub fn take_pending_key() -> Option<i32> {
    PANELS.with(|p| p.borrow_mut().pending_key.take())
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
        if s.alert_hl && s.firing.contains(&pid) {
            Some(hot_row_attr())
        } else {
            None
        }
    })
}

/// The full-row color for a firing (over-threshold) process: bold white on a
/// red background. Deliberately NOT the cyan of `PANEL_SELECTION_FOCUS`, so a
/// hot row can never be mistaken for the cursor, and high-contrast so the text
/// stays legible over the fill — unlike htop's `FAILED_SEARCH` (red-on-cyan),
/// which both collides with the selection color and reads poorly. `Panel_draw`
/// still lets the cursor's selection highlight win on the selected row, so the
/// cursor stays visible even over a hot row. Falls back to reverse+bold on the
/// monochrome scheme, where no colors exist.
fn hot_row_attr() -> i32 {
    use crate::ported::crt::{ColorPair, Red, White, A_BOLD, A_REVERSE};
    if matches!(ColorScheme::active(), ColorScheme::COLORSCHEME_MONOCHROME) {
        A_REVERSE | A_BOLD
    } else {
        A_BOLD | ColorPair(White, Red)
    }
}

/// The process panel's `rowHeight` signal: `1 + SPARK_GRAPH_H` (the maximum a
/// process can occupy) when the `v` cycle is in double-height mode, else `1`.
/// A value `> 1` tells `Panel_draw`/`Panel_onKey` the panel is in graph mode and
/// must use per-row heights ([`graph_lines`]); the actual lines a given row
/// takes are CPU-dependent, so this is only the ceiling.
pub fn row_height() -> i32 {
    PANELS.with(|p| {
        if p.borrow().spark == SparkMode::Double {
            1 + SPARK_GRAPH_H
        } else {
            1
        }
    })
}

/// Graph lines a process occupies beneath its info row: `0` when not in
/// double-height mode, otherwise scaled by the PID's latest CPU so busier
/// processes are taller ("more CPU = more rows"). Clamped to `1..=SPARK_GRAPH_H`
/// for any live CPU, `0` only at exactly idle. The process's total row height is
/// `1 + graph_lines(pid)`.
pub fn graph_lines(pid: u32) -> i32 {
    PANELS.with(|p| {
        let s = p.borrow();
        if s.spark != SparkMode::Double {
            return 0;
        }
        let cpu = s.ring.latest_cpu(pid);
        if cpu <= 0.0 {
            0
        } else {
            ((cpu / 100.0 * SPARK_GRAPH_H as f32).ceil() as i32).clamp(1, SPARK_GRAPH_H)
        }
    })
}

/// Overdraw the CPU sparkline column at the right edge of a process row, when
/// the `v` cycle is in its side-column state. Called by `Panel_draw` after it
/// prints the row, with the row's htop-space `(y, x)` and panel width `w` (the
/// `Ncurses` shim adds the border margin). A no-op when the column is off or the
/// PID has no history.
pub fn draw_spark_col<W: Write>(out: &mut W, y: i32, x: i32, w: i32, pid: u32) {
    PANELS.with(|p| {
        let s = p.borrow();
        if s.spark != SparkMode::Column || w as usize <= SPARK_W + 2 {
            return;
        }
        // One braille row `SPARK_W` cells wide (same renderer as the `G`
        // graph). Print the whole string — `mvaddnstr`'s `n` is a *byte* limit
        // and braille glyphs are 3 bytes each, so an `n`-slice would land
        // mid-char and panic.
        let spark = s
            .ring
            .cpu_braille(pid, SPARK_W, 1, 100.0)
            .into_iter()
            .next()
            .unwrap_or_default();
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

/// Draw the full-width CPU graph on a process row's `n_rows` physical lines
/// beneath its info line (double-height mode). Called by `Panel_draw` with the
/// graph's top htop-space `(y_top, x)`, the panel width `w`, and how many lines
/// the graph spans (`1 + graph_lines`-driven, so busier processes are taller).
/// A multi-row braille bitmap like the `G` graph, newest sample at the right,
/// bars growing up from the bottom. A no-op when not in double-height mode.
pub fn draw_spark_row<W: Write>(out: &mut W, y_top: i32, x: i32, w: i32, n_rows: i32, pid: u32) {
    PANELS.with(|p| {
        let s = p.borrow();
        if s.spark != SparkMode::Double || w <= 0 || n_rows <= 0 {
            return;
        }
        // `n_rows` braille rows spanning the full panel width (same renderer as
        // the `G` graph): each cell is 2 samples wide, the newest at the right
        // edge, bars growing up across all `n_rows` lines.
        let rows = s.ring.cpu_braille(pid, w as usize, n_rows as usize, 100.0);
        Ncurses::attrset(
            out,
            ColorElements::PROCESS_MEGABYTES.packed(ColorScheme::active()),
        );
        for (i, row) in rows.iter().enumerate() {
            Ncurses::mvaddstr(out, y_top + i as i32, x, row);
        }
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
    fn aggregate_opens_cycles_and_closes() {
        ingest_ticks(1);
        assert!(!panel_active());
        assert!(dispatch_key(0x79)); // 'y' opens the aggregate modal
        assert!(panel_active());
        let first = PANELS.with(|p| p.borrow().agg_by);
        dispatch_key(9); // Tab cycles the group-by key
        let second = PANELS.with(|p| p.borrow().agg_by);
        assert_ne!(first, second);
        assert_eq!(second, first.next()); // cycles in order, and persists
        dispatch_key(27); // Esc closes
        assert!(!panel_active());
    }

    #[test]
    fn palette_matches_and_injects_action_key() {
        ingest_ticks(1);
        assert!(dispatch_key(0x3a)); // ':' opens the command palette
        assert!(panel_active());
        for b in b"aggreg" {
            dispatch_key(*b as i32);
        }
        // The aggregate command is the top fuzzy hit for "aggreg".
        let top = PANELS.with(|p| p.borrow().palette_hits.first().map(|m| CMDS[m.idx].1));
        assert_eq!(top, Some(b'y' as i32));
        dispatch_key(13); // Enter runs it: queues the key, closes the modal
        assert!(!panel_active());
        assert_eq!(take_pending_key(), Some(b'y' as i32));
        // Consumed once.
        assert_eq!(take_pending_key(), None);
    }

    #[test]
    fn palette_reaches_htop_actions_too() {
        // "kill" resolves to htop's own 'k'; injection isn't limited to
        // extension actions.
        ingest_ticks(1);
        dispatch_key(0x3a);
        for b in b"kill" {
            dispatch_key(*b as i32);
        }
        let top = PANELS.with(|p| p.borrow().palette_hits.first().map(|m| CMDS[m.idx].1));
        assert_eq!(top, Some(b'k' as i32));
        dispatch_key(27); // Esc cancels (no injection)
        assert!(!panel_active());
        assert_eq!(take_pending_key(), None);
    }

    #[test]
    fn aggregate_rolls_up_synthetic_table() {
        // The pure rollup sums CPU/mem and returns at least one group for the
        // synthetic table, sorted CPU-descending.
        ingest_ticks(1);
        let groups = PANELS.with(|p| {
            aggregate(
                &p.borrow().table,
                crate::extensions::aggregate::GroupBy::User,
            )
        });
        assert!(!groups.is_empty());
        for w in groups.windows(2) {
            assert!(w[0].cpu >= w[1].cpu, "groups must be CPU-descending");
        }
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
        // 'v' cycles Off → Column → Double → Off; `row_height()` tracks it.
        assert_eq!(PANELS.with(|p| p.borrow().spark), SparkMode::Off);
        assert_eq!(row_height(), 1);
        dispatch_key(0x76);
        assert_eq!(PANELS.with(|p| p.borrow().spark), SparkMode::Column);
        assert_eq!(row_height(), 1); // side column keeps single-height rows
        dispatch_key(0x76);
        assert_eq!(PANELS.with(|p| p.borrow().spark), SparkMode::Double);
        assert_eq!(row_height(), 1 + SPARK_GRAPH_H); // graph-mode ceiling
        dispatch_key(0x76);
        assert_eq!(PANELS.with(|p| p.borrow().spark), SparkMode::Off);
        assert_eq!(row_height(), 1);
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
        // The hot-row highlight is off by default (opt-in); enable it to check
        // the recolor path.
        PANELS.with(|p| p.borrow_mut().alert_hl = true);
        assert!(alert_attr(999).is_some());
        assert!(alert_attr(1).is_none());
        // Regression: the firing-row recolor must NOT be the cursor's selection
        // color. It previously used FAILED_SEARCH (red-on-cyan), whose cyan
        // background matched PANEL_SELECTION_FOCUS, so hot rows looked like the
        // cursor and hid it. The hot color must stay distinct.
        let selection = ColorElements::PANEL_SELECTION_FOCUS.packed(ColorScheme::active());
        assert_ne!(alert_attr(999), Some(selection));
        assert_eq!(alert_attr(999), Some(hot_row_attr()));
        // Toggling the highlight off (Alerts modal 't') suppresses the recolor
        // while the engine keeps firing, then back on restores it.
        PANELS.with(|p| p.borrow_mut().alert_hl = false);
        assert!(alert_attr(999).is_none());
        assert_eq!(PANELS.with(|p| p.borrow().firing.len()), 1);
        PANELS.with(|p| p.borrow_mut().alert_hl = true);
        assert!(alert_attr(999).is_some());
    }

    #[test]
    fn spark_col_never_panics_on_braille() {
        // Regression: the braille sparkline is 3-byte glyphs; a byte-length
        // `mvaddnstr` slice landed mid-char and panicked. Enable the column,
        // build up history, and render into a sink — must not panic.
        PANELS.with(|p| p.borrow_mut().spark = SparkMode::Column);
        for t in 0..40 {
            PANELS.with(|p| p.borrow_mut().ingest(synthetic_table(t), Some(200)));
        }
        let mut sink: Vec<u8> = Vec::new();
        // pid 200 has ~40 samples -> a full-width multi-byte sparkline.
        draw_spark_col(&mut sink, 3, 0, 80, 200);
        assert!(!sink.is_empty());
        PANELS.with(|p| p.borrow_mut().spark = SparkMode::Off); // leave state clean
    }

    #[test]
    fn spark_row_full_width_never_panics() {
        // The double-height graph renders `n_rows` full panel-width braille
        // lines; multi-byte braille + a wide `w` must not panic or mis-slice.
        PANELS.with(|p| p.borrow_mut().spark = SparkMode::Double);
        for t in 0..40 {
            PANELS.with(|p| p.borrow_mut().ingest(synthetic_table(t), Some(200)));
        }
        assert_eq!(row_height(), 1 + SPARK_GRAPH_H);
        let mut sink: Vec<u8> = Vec::new();
        draw_spark_row(&mut sink, 4, 0, 120, SPARK_GRAPH_H, 200);
        assert!(!sink.is_empty());
        // Off mode draws nothing.
        PANELS.with(|p| p.borrow_mut().spark = SparkMode::Off);
        let mut empty: Vec<u8> = Vec::new();
        draw_spark_row(&mut empty, 4, 0, 120, SPARK_GRAPH_H, 200);
        assert!(empty.is_empty());
    }

    #[test]
    fn hotkeys_emit_confirmation_toasts() {
        use crate::extensions::overlay::status_text;
        // Every idle monitoring hotkey sets a confirmation toast, like `b`.
        PANELS.with(|p| p.borrow_mut().spark = SparkMode::Off);
        dispatch_key(0x76); // 'v' — spark cycle
        assert_eq!(status_text().as_deref(), Some("CPU graph: column"));
        dispatch_key(0x47); // 'G' — graph
        assert_eq!(status_text().as_deref(), Some("CPU history graph"));
        dispatch_key(27); // close modal
        dispatch_key(0x6f); // 'o' — export (succeeds or fails by env, but toasts)
        assert!(status_text().is_some(), "export must toast");
        dispatch_key(27);
        PANELS.with(|p| p.borrow_mut().spark = SparkMode::Off); // reset shared state
    }

    #[test]
    fn graph_lines_scale_with_cpu() {
        PANELS.with(|p| p.borrow_mut().spark = SparkMode::Double);
        let mk = |pid: u32, cpu: f32| Proc {
            pid,
            ppid: 1,
            user: "u".into(),
            comm: "c".into(),
            cmdline: "c".into(),
            state: 'R',
            cpu,
            mem_kb: 1,
        };
        PANELS.with(|p| {
            p.borrow_mut()
                .ingest(vec![mk(10, 0.0), mk(11, 20.0), mk(12, 100.0)], None)
        });
        // idle: no graph rows; light load: 1 row; pegged: the full ceiling.
        assert_eq!(graph_lines(10), 0);
        assert_eq!(graph_lines(11), 1);
        assert_eq!(graph_lines(12), SPARK_GRAPH_H);
        // Off mode: always zero regardless of CPU.
        PANELS.with(|p| p.borrow_mut().spark = SparkMode::Off);
        assert_eq!(graph_lines(12), 0);
    }
}
