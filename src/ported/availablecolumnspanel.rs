//! Partial port of `AvailableColumnsPanel.c` — the picker Panel listing
//! every column a user can add to a process screen.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module — matching the spec
//! name-for-name is the point of the port.
//!
//! # Struct model
//!
//! htop's `AvailableColumnsPanel` (`AvailableColumnsPanel.h:14`) embeds a
//! `Panel super` plus a `Panel* columns` back-pointer to the `ColumnsPanel`
//! that spawned this picker. The reduced [`AvailableColumnsPanel`] struct
//! models only the embedded `super_` [`Panel`]; the `columns` alias — a
//! foreign-owned mutable `Panel*` — is **omitted**, exactly as
//! `screenspanel.rs` omits its `columns`/`availableColumns`/`scr`
//! back-pointers (and `columnspanel.rs` its `ss`). The functions that
//! dereference `columns` stay stubbed for that reason.
//!
//! # Now ported
//!
//! - [`AvailableColumnsPanel_addDynamicColumn`] — the `Hashtable_foreach`
//!   callback. All the `DynamicColumn` fields it reads (`table`, `heading`,
//!   `name`, `description`, `caption`) are now modeled in
//!   `dynamiccolumn.rs`, and `Panel_add` / `ListItem_new` are ported.
//! - [`AvailableColumnsPanel_addDynamicColumns`] — drives the ported
//!   [`Hashtable_foreach`] over the ported [`Hashtable`] with the callback
//!   above.
//!
//! # Still stubbed
//!
//! The remaining blockers:
//!
//! - **`Process_fields[]`** — the platform-provided
//!   `const ProcessFieldData Process_fields[]` table (`.name` /
//!   `.description` per column) is still unported (`settings.rs` /
//!   `columnspanel.rs` record it as missing substrate). Blocks
//!   [`AvailableColumnsPanel_insert`] and
//!   [`AvailableColumnsPanel_addPlatformColumns`].
//! - **`LAST_PROCESSFIELD` / `ROW_DYNAMIC_FIELDS`** (`RowField.h:53`) — the
//!   reserved-field count bounds are still not modeled. Blocks
//!   [`AvailableColumnsPanel_insert`] and
//!   [`AvailableColumnsPanel_addPlatformColumns`].
//! - **`DynamicColumn_name`** (`DynamicColumn.c:36`) — still a `todo!()`
//!   stub. Blocks [`AvailableColumnsPanel_insert`].
//! - **`Platform_addDynamicScreenAvailableColumns`** — the `Platform`
//!   layer is not ported. Blocks [`AvailableColumnsPanel_addDynamicScreens`].
//! - **The omitted `columns: Panel*` back-pointer** — blocks
//!   [`AvailableColumnsPanel_insert`], [`AvailableColumnsPanel_eventHandler`]
//!   (both mutate through it), and [`AvailableColumnsPanel_new`] (stores it).
//!
//! Stubbed (cannot be ported faithfully yet):
//! - [`AvailableColumnsPanel_delete`] (`AvailableColumnsPanel.c:32`) —
//!   `Panel_done` + `free`; in Rust the owned fields are released by
//!   `Drop`, so there is no algorithm to port (same class as
//!   `Panel_delete` / `ListItem_delete`).
//! - [`AvailableColumnsPanel_insert`] (`AvailableColumnsPanel.c:38`).
//! - [`AvailableColumnsPanel_eventHandler`] (`AvailableColumnsPanel.c:47`).
//! - [`AvailableColumnsPanel_addPlatformColumns`] (`AvailableColumnsPanel.c:105`).
//! - [`AvailableColumnsPanel_addDynamicScreens`] (`AvailableColumnsPanel.c:116`).
//! - [`AvailableColumnsPanel_fill`] (`AvailableColumnsPanel.c:120`).
//! - [`AvailableColumnsPanel_new`] (`AvailableColumnsPanel.c:130`).
#![allow(non_snake_case)]
#![allow(dead_code)]

use crate::ported::dynamiccolumn::DynamicColumn;
use crate::ported::hashtable::{Hashtable, Hashtable_foreach};
use crate::ported::listitem::ListItem_new;
use crate::ported::panel::{Panel, Panel_add};

/// Port of `typedef struct AvailableColumnsPanel_` (`AvailableColumnsPanel.h:14`).
///
/// The C struct is `{ Panel super; Panel* columns; }`. The `columns`
/// back-pointer is a foreign-owned mutable `Panel*` (owned by the spawning
/// `ColumnsPanel`); no ported panel models such a cross-owned alias, so —
/// following the `screenspanel.rs` / `columnspanel.rs` reduced-struct
/// precedent — it is omitted here and the functions that dereference it
/// remain stubs.
pub struct AvailableColumnsPanel {
    /// C `Panel super` — the embedded panel base. `super_` avoids the Rust
    /// `super` keyword, matching the `columnspanel.rs` convention.
    pub super_: Panel,
}

/// TODO: port of `static void AvailableColumnsPanel_delete(Object* object)`
/// from `AvailableColumnsPanel.c:32`. Pure teardown (`Panel_done(&this->super);
/// free(this);`) — the Rust struct would own its fields and free them via
/// `Drop`, so there is no algorithm to port (same class as `Panel_delete` /
/// `ListItem_delete`, both left as stubs).
pub fn AvailableColumnsPanel_delete() {
    todo!("port of AvailableColumnsPanel.c:32 — Drop releases the panel")
}

/// TODO: port of `static void AvailableColumnsPanel_insert(AvailableColumnsPanel* this,
/// int at, int key)` from `AvailableColumnsPanel.c:38`. Chooses the column
/// name via `key >= ROW_DYNAMIC_FIELDS ? DynamicColumn_name(key) :
/// Process_fields[key].name` and inserts a `ListItem_new(name, key)` into
/// `this->columns`. Blocked on the unported `Process_fields[]` table,
/// `ROW_DYNAMIC_FIELDS` (`RowField.h:53`), the stubbed `DynamicColumn_name`
/// (`DynamicColumn.c:36`), and the omitted foreign-owned `columns: Panel*`
/// back-pointer.
pub fn AvailableColumnsPanel_insert() {
    todo!("port of AvailableColumnsPanel.c:38 — needs Process_fields[]/ROW_DYNAMIC_FIELDS/DynamicColumn_name + columns Panel*")
}

/// TODO: port of `static HandlerResult AvailableColumnsPanel_eventHandler(Panel* super,
/// int ch)` from `AvailableColumnsPanel.c:47`. On Enter/reclick/F5 it inserts
/// the selected column into `this->columns` and calls `ColumnsPanel_update`;
/// otherwise it falls through to `Panel_selectByTyping`. `HandlerResult`,
/// `Panel_selectByTyping` (`Panel.c:468`), and `ColumnsPanel_update`
/// (`ColumnsPanel.c:181`) are all ported now, but this is still blocked on the
/// omitted foreign-owned `columns: Panel*` back-pointer it mutates through and
/// on the stubbed [`AvailableColumnsPanel_insert`] it calls.
pub fn AvailableColumnsPanel_eventHandler() {
    todo!("port of AvailableColumnsPanel.c:47 — needs columns Panel* back-pointer + AvailableColumnsPanel_insert")
}

/// Port of `static void AvailableColumnsPanel_addDynamicColumn(ht_key_t key,
/// void* value, void* data)` from `AvailableColumnsPanel.c:83`. A
/// [`Hashtable_foreach`] callback that skips `DynamicScreen` columns (those
/// with a non-null `column->table`), formats `"<heading|name> -
/// <description|caption>"` (or just the title when there is no text), and
/// `Panel_add`s a `ListItem_new(description, key)`.
///
/// The C signature is the raw `(ht_key_t, void* value, void* data)` callback
/// shape. Following the [`crate::ported::dynamiccolumn::DynamicColumn_compare`]
/// precedent, the port takes the already-downcast `column: &DynamicColumn`
/// (C's `value`) and `this: &mut AvailableColumnsPanel` (C's `data`) directly;
/// [`AvailableColumnsPanel_addDynamicColumns`] performs the `void*` downcast
/// inside its `Hashtable_foreach` closure, exactly as C's `(const
/// DynamicColumn*) value` / `(AvailableColumnsPanel*) data` casts do.
///
/// C's `xSnprintf` into a fixed `char description[256]` becomes a heap
/// `format!`; the 256-byte truncation is a fixed-buffer artifact, not part of
/// the column-listing semantics. `ht_key_t` is C `unsigned int`, so `key` is
/// cast to the `i32` [`ListItem_new`] takes.
pub fn AvailableColumnsPanel_addDynamicColumn(
    key: u32,
    column: &DynamicColumn,
    this: &mut AvailableColumnsPanel,
) {
    // C: if (column->table) return; /* DynamicScreen, handled differently */
    if !column.table.is_null() {
        return;
    }
    // C: const char* title = column->heading ? column->heading : column->name;
    let title: &str = column.heading.as_deref().unwrap_or(&column.name);
    // C: const char* text = column->description ? column->description : column->caption;
    let text: Option<&str> = column.description.as_deref().or(column.caption.as_deref());
    // C: char description[256];
    //    if (text) xSnprintf(..., "%s - %s", title, text); else xSnprintf(..., "%s", title);
    let description = match text {
        Some(text) => format!("{title} - {text}"),
        None => title.to_string(),
    };
    // C: Panel_add(&this->super, (Object*) ListItem_new(description, key));
    Panel_add(
        &mut this.super_,
        Box::new(ListItem_new(&description, key as i32)),
    );
}

/// Port of `static void AvailableColumnsPanel_addDynamicColumns(AvailableColumnsPanel* this,
/// Hashtable* dynamicColumns)` from `AvailableColumnsPanel.c:99`. Drives
/// `Hashtable_foreach(dynamicColumns, AvailableColumnsPanel_addDynamicColumn,
/// this)`.
///
/// C's `assert(dynamicColumns)` guards a non-null pointer; a `&Hashtable` is
/// always valid in Rust, so the assertion is subsumed by the reference type
/// (the same treatment `DynamicColumn_search`'s `if (dynamics)` guard gets).
/// The ported [`Hashtable_foreach`] hands each value to a `FnMut` closure as
/// a `&dyn Object`; the closure downcasts it to `&DynamicColumn` (C's `(const
/// DynamicColumn*) value` cast) and forwards to the free-fn callback.
pub fn AvailableColumnsPanel_addDynamicColumns(
    this: &mut AvailableColumnsPanel,
    dynamicColumns: &Hashtable,
) {
    // C: assert(dynamicColumns); -- subsumed by the &Hashtable reference type.
    // C: Hashtable_foreach(dynamicColumns, AvailableColumnsPanel_addDynamicColumn, this);
    Hashtable_foreach(dynamicColumns, &mut |key, value| {
        let any: &dyn core::any::Any = value;
        let column = any.downcast_ref::<DynamicColumn>().expect(
            "AvailableColumnsPanel_addDynamicColumns: hashtable value is not a DynamicColumn",
        );
        AvailableColumnsPanel_addDynamicColumn(key, column, this);
    });
}

/// TODO: port of `static void AvailableColumnsPanel_addPlatformColumns(AvailableColumnsPanel* this)`
/// from `AvailableColumnsPanel.c:105`. Loops `1..LAST_PROCESSFIELD`, and for
/// each field with a description, formats `"<name> - <description>"` and
/// `Panel_add`s a `ListItem_new`. Blocked on the unported `Process_fields[]`
/// table and the `LAST_PROCESSFIELD` bound.
pub fn AvailableColumnsPanel_addPlatformColumns() {
    todo!("port of AvailableColumnsPanel.c:105 — needs Process_fields[] + LAST_PROCESSFIELD")
}

/// TODO: port of `static void AvailableColumnsPanel_addDynamicScreens(AvailableColumnsPanel* this,
/// const char* screen)` from `AvailableColumnsPanel.c:116`. Delegates to
/// `Platform_addDynamicScreenAvailableColumns(&this->super, screen)`. Blocked
/// on the unported `Platform` layer.
pub fn AvailableColumnsPanel_addDynamicScreens() {
    todo!("port of AvailableColumnsPanel.c:116 — needs Platform_addDynamicScreenAvailableColumns")
}

/// TODO: port of `void AvailableColumnsPanel_fill(AvailableColumnsPanel* this,
/// const char* dynamicScreen, Hashtable* dynamicColumns)` from
/// `AvailableColumnsPanel.c:120`. `Panel_prune`s the panel, then either
/// `addDynamicScreens` (dynamic screen) or `addPlatformColumns` +
/// `addDynamicColumns`. `Panel_prune` and [`AvailableColumnsPanel_addDynamicColumns`]
/// are ported, but the other two dispatch targets
/// ([`AvailableColumnsPanel_addPlatformColumns`] and
/// [`AvailableColumnsPanel_addDynamicScreens`]) are still blocked stubs — a
/// body dispatching to them would panic on every branch — so this stays a stub
/// (the `screenspanel.rs` `addNewScreen`-dispatches-to-a-stub precedent).
pub fn AvailableColumnsPanel_fill() {
    todo!("port of AvailableColumnsPanel.c:120 — dispatch targets addPlatformColumns/addDynamicScreens still blocked")
}

/// TODO: port of `AvailableColumnsPanel* AvailableColumnsPanel_new(Panel* columns,
/// Hashtable* dynamicColumns)` from `AvailableColumnsPanel.c:130`. Allocates
/// the panel, builds its `FunctionBar_new`, `Panel_init`s it, sets the
/// "Available Columns" header, stores the `columns` back-pointer, and calls
/// `AvailableColumnsPanel_fill`. `FunctionBar_new` / `Panel_init` /
/// `Panel_setHeader` are ported, but this is blocked on the omitted
/// foreign-owned `columns: Panel*` field (`this->columns = columns`, no ported
/// cross-owned-panel alias) and on the stubbed [`AvailableColumnsPanel_fill`]
/// it must call to populate the list.
pub fn AvailableColumnsPanel_new() {
    todo!("port of AvailableColumnsPanel.c:130 — needs columns Panel* field + AvailableColumnsPanel_fill")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ported::listitem::ListItem;
    use crate::ported::panel::{Panel_new, Panel_size};
    use crate::ported::table::Table;

    fn panel() -> AvailableColumnsPanel {
        AvailableColumnsPanel {
            super_: Panel_new(0, 0, 1, 1, None),
        }
    }

    fn column(
        name: &str,
        heading: Option<&str>,
        description: Option<&str>,
        caption: Option<&str>,
        table: *const Table,
    ) -> DynamicColumn {
        DynamicColumn {
            name: name.to_string(),
            heading: heading.map(str::to_string),
            caption: caption.map(str::to_string),
            description: description.map(str::to_string),
            width: 0,
            enabled: true,
            table,
        }
    }

    fn item_at(this: &AvailableColumnsPanel, i: usize) -> (&str, i32) {
        let obj: &dyn core::any::Any = this.super_.items[i].as_ref();
        let li = obj
            .downcast_ref::<ListItem>()
            .expect("panel item is not a ListItem");
        (li.value.as_str(), li.key)
    }

    #[test]
    fn addDynamicColumn_uses_heading_and_description() {
        let mut p = panel();
        let col = column(
            "io_rate",
            Some("IO"),
            Some("disk io rate"),
            None,
            core::ptr::null(),
        );
        AvailableColumnsPanel_addDynamicColumn(7, &col, &mut p);
        assert_eq!(Panel_size(&p.super_), 1);
        assert_eq!(item_at(&p, 0), ("IO - disk io rate", 7));
    }

    #[test]
    fn addDynamicColumn_falls_back_to_name_and_caption() {
        let mut p = panel();
        // No heading -> title is name; no description -> text is caption.
        let col = column("io_rate", None, None, Some("io"), core::ptr::null());
        AvailableColumnsPanel_addDynamicColumn(3, &col, &mut p);
        assert_eq!(item_at(&p, 0), ("io_rate - io", 3));
    }

    #[test]
    fn addDynamicColumn_no_text_uses_title_only() {
        let mut p = panel();
        let col = column("io_rate", Some("IO"), None, None, core::ptr::null());
        AvailableColumnsPanel_addDynamicColumn(1, &col, &mut p);
        assert_eq!(item_at(&p, 0), ("IO", 1));
    }

    #[test]
    fn addDynamicColumn_skips_dynamicscreen_columns() {
        let mut p = panel();
        // A non-null table marks a DynamicScreen column: it must be skipped.
        // The pointer is only null-checked, never dereferenced.
        let table = core::ptr::NonNull::<Table>::dangling().as_ptr() as *const Table;
        let col = column("io_rate", Some("IO"), Some("desc"), None, table);
        AvailableColumnsPanel_addDynamicColumn(9, &col, &mut p);
        assert_eq!(Panel_size(&p.super_), 0);
    }

    #[test]
    fn addDynamicColumns_adds_every_non_screen_column() {
        use crate::ported::hashtable::{Hashtable_new, Hashtable_put};

        let mut ht = Hashtable_new(0, true);
        Hashtable_put(
            &mut ht,
            5,
            Box::new(column(
                "cpu",
                Some("CPU"),
                Some("cpu usage"),
                None,
                core::ptr::null(),
            )),
        );
        let mut p = panel();
        AvailableColumnsPanel_addDynamicColumns(&mut p, &ht);
        assert_eq!(Panel_size(&p.super_), 1);
        assert_eq!(item_at(&p, 0), ("CPU - cpu usage", 5));
    }
}
