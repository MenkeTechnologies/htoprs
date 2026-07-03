//! Real `Process` → extension [`Proc`] bridge.
//!
//! The monitoring extensions ([`super::procring`], [`super::alerts`],
//! [`super::filter`], [`super::snapshot`], [`super::export`], [`super::graph`])
//! were prototyped against [`super::model::Proc`], a stand-in for the ported
//! `Process`. This module reads the running [`Table`]'s live rows off the real
//! `Process` and materializes them as `Proc`, so the same extension engines run
//! on real system data instead of the synthetic generator.
//!
//! Field extraction mirrors `Process_writeField` (`process.rs`): pid/ppid come
//! from the embedded `Row` via the getters, `state` through `processStateChar`,
//! the rest are direct `Process` fields. Rows that are not processes (a panel
//! may hold other `Object`s) are skipped by `Object::as_process`.

use crate::extensions::model::Proc;
use crate::ported::panel::Panel;
use crate::ported::process::{processStateChar, Process, Process_getParent, Process_getPid};
use crate::ported::table::Table;

/// Materialize one real `Process` as a `Proc` row.
pub fn proc_from(p: &Process) -> Proc {
    Proc {
        pid: Process_getPid(p).max(0) as u32,
        ppid: Process_getParent(p).max(0) as u32,
        // htop falls back to the numeric uid when the name is unresolved
        // (`Process_writeField` USER arm).
        user: p.user.clone().unwrap_or_else(|| p.st_uid.to_string()),
        comm: p.procComm.clone().unwrap_or_default(),
        cmdline: p.cmdline.clone().unwrap_or_default(),
        state: processStateChar(p.state),
        cpu: p.percent_cpu,
        // `m_resident` is a signed KiB count; clamp the (never-negative in
        // practice) value into the unsigned `Proc` field.
        mem_kb: p.m_resident.max(0) as u64,
    }
}

/// Snapshot every process row of the live table, in `rows` order.
///
/// `rows` (not `displayList`) is used so the engines see the full set the way
/// `ProcessTable_scanEntries` does — history/alert tracking must not blink when
/// the tree view collapses a branch out of the display list.
pub fn snapshot_table(table: &Table) -> Vec<Proc> {
    table
        .rows
        .iter()
        .flatten()
        .filter_map(|o| o.as_process())
        .map(proc_from)
        .collect()
}

/// The pid of the currently-selected row, read from the table's panel.
///
/// Returns `None` when the table has no panel wired yet, the selection is out
/// of range, or the selected row is not a process.
pub fn selected_pid(table: &Table) -> Option<u32> {
    if table.panel.is_null() {
        return None;
    }
    // SAFETY: `Table::panel` aliases the caller-owned `Panel` for the run, the
    // same pointer `Table_rebuildPanel` writes and reads (`table.rs`).
    let panel = unsafe { &*table.panel };
    index_of_selected(panel)
}

/// The pid under the panel's cursor (`selected`), if that item is a process.
fn index_of_selected(panel: &Panel) -> Option<u32> {
    let i = panel.selected;
    if i < 0 || i as usize >= panel.items.len() {
        return None;
    }
    panel.items[i as usize]
        .object()
        .as_process()
        .map(|p| Process_getPid(p).max(0) as u32)
}

/// The panel item index whose row is `pid`, for driving `Panel_setSelected`
/// (finder "jump to match"). Scans in panel order; returns the first hit.
pub fn index_of_pid(panel: &Panel, pid: u32) -> Option<i32> {
    panel.items.iter().enumerate().find_map(|(i, it)| {
        it.object()
            .as_process()
            .filter(|p| Process_getPid(p).max(0) as u32 == pid)
            .map(|_| i as i32)
    })
}
