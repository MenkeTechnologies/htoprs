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
//! that spawned this picker. The [`AvailableColumnsPanel`] struct models the
//! embedded `super_` [`Panel`] and the `columns` alias as a raw `*mut Panel`
//! (the `ColorsPanel`/`HeaderOptionsPanel` back-pointer idiom — the
//! `ColumnsPanel`'s panel is owned elsewhere).
//!
//! # Now ported
//!
//! - [`AvailableColumnsPanel_addDynamicColumn`] / [`AvailableColumnsPanel_addDynamicColumns`]
//!   — the `Hashtable_foreach` callback and its driver.
//! - [`AvailableColumnsPanel_insert`] / [`AvailableColumnsPanel_eventHandler`]
//!   — insert the selected column into `this->columns` (`Process_fields[]` /
//!   `DynamicColumn_name` / `ROW_DYNAMIC_FIELDS` are all modeled now).
//! - [`AvailableColumnsPanel_addPlatformColumns`] — lists every
//!   `Process_fields[i]` with a description.
//! - [`AvailableColumnsPanel_fill`] / [`AvailableColumnsPanel_new`] — prune +
//!   repopulate, and the constructor.
//!
//! # Still stubbed
//!
//! - [`AvailableColumnsPanel_delete`] (`AvailableColumnsPanel.c:32`) —
//!   `Panel_done` + `free`; in Rust the owned fields are released by `Drop`,
//!   so there is no algorithm to port (same class as `Panel_delete` /
//!   `ListItem_delete`).
//!
//! # Now ported (was stubbed)
//!
//! - [`AvailableColumnsPanel_addDynamicScreens`] (`AvailableColumnsPanel.c:116`)
//!   — delegates to [`Platform_addDynamicScreenAvailableColumns`], a non-PCP
//!   `static inline` no-op (`linux/Platform.h:162`), now provided per platform.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use crate::ported::columnspanel::ColumnsPanel_update;
use crate::ported::crt::{KEY_ENTER, KEY_F, KEY_RECLICK};
use crate::ported::dynamiccolumn::{DynamicColumn, DynamicColumn_name};
use crate::ported::functionbar::FunctionBar_new;
use crate::ported::hashtable::{Hashtable, Hashtable_foreach};
use crate::ported::linux::linuxprocess::{Process_fields, LAST_PROCESSFIELD};
use crate::ported::listitem::{ListItem, ListItem_new};
use crate::ported::panel::{
    HandlerResult, Panel, PanelClass, Panel_add, Panel_done, Panel_getSelected,
    Panel_getSelectedIndex, Panel_insert, Panel_new, Panel_prune, Panel_selectByTyping,
    Panel_setHeader, Panel_setSelected,
};

#[cfg(target_os = "macos")]
use crate::ported::darwin::platform::Platform_addDynamicScreenAvailableColumns;
#[cfg(target_os = "dragonfly")]
use crate::ported::dragonflybsd::platform::Platform_addDynamicScreenAvailableColumns;
#[cfg(target_os = "freebsd")]
use crate::ported::freebsd::platform::Platform_addDynamicScreenAvailableColumns;
#[cfg(target_os = "linux")]
use crate::ported::linux::platform::Platform_addDynamicScreenAvailableColumns;
#[cfg(target_os = "netbsd")]
use crate::ported::netbsd::platform::Platform_addDynamicScreenAvailableColumns;
#[cfg(target_os = "openbsd")]
use crate::ported::openbsd::platform::Platform_addDynamicScreenAvailableColumns;
#[cfg(any(target_os = "solaris", target_os = "illumos"))]
use crate::ported::solaris::platform::Platform_addDynamicScreenAvailableColumns;
#[cfg(not(any(
    target_os = "macos",
    target_os = "linux",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "solaris",
    target_os = "illumos",
    target_os = "dragonfly"
)))]
use crate::ported::unsupported::platform::Platform_addDynamicScreenAvailableColumns;

/// Port of `#define ROW_DYNAMIC_FIELDS LAST_RESERVED_FIELD` (`RowField.h:53`).
/// `LAST_RESERVED_FIELD == LAST_PROCESSFIELD` (`Process.h:229`), the reserved
/// (non-dynamic) field count.
const ROW_DYNAMIC_FIELDS: i32 = LAST_PROCESSFIELD as i32;

/// Port of the file-scope
/// `static const char* const AvailableColumnsFunctions[]` from
/// `AvailableColumnsPanel.c:29`. `F5=Add`, `F10=Done`, the rest blank; the C
/// trailing `NULL` sentinel is dropped (the ported `FunctionBar_new` is
/// length-bounded, not NUL-terminated).
static AvailableColumnsFunctions: [&str; 10] = [
    "      ", "      ", "      ", "      ", "Add   ", "      ", "      ", "      ", "      ",
    "Done  ",
];

/// Port of `typedef struct AvailableColumnsPanel_` (`AvailableColumnsPanel.h:14`).
///
/// The C struct is `{ Panel super; Panel* columns; }`. `columns` is a
/// foreign-owned mutable `Panel*` (owned by the spawning `ColumnsPanel`),
/// modeled here as a raw `*mut Panel` — the `ColorsPanel`/`HeaderOptionsPanel`
/// back-pointer idiom (the `ColumnsPanel`'s panel is owned elsewhere).
pub struct AvailableColumnsPanel {
    /// C `Panel super` — the embedded panel base. `super_` avoids the Rust
    /// `super` keyword, matching the `columnspanel.rs` convention.
    pub super_: Panel,
    /// C `Panel* columns` — non-owning back-pointer to the `ColumnsPanel`'s
    /// panel that this picker inserts into.
    pub columns: *mut Panel,
}

/// Port of `AvailableColumnsPanel.c`'s `const PanelClass
/// AvailableColumnsPanel_class` vtable (`AvailableColumnsPanel.c:75`). C sets
/// only `.eventHandler = AvailableColumnsPanel_eventHandler`; `.drawFunctionBar`
/// / `.printHeader` are NULL, so those slots inherit the trait defaults. Wires
/// `event_handler` to [`AvailableColumnsPanel_eventHandler`].
impl PanelClass for AvailableColumnsPanel {
    fn as_panel(&self) -> &Panel {
        &self.super_
    }
    fn as_panel_mut(&mut self) -> &mut Panel {
        &mut self.super_
    }
    fn event_handler(&mut self, ev: i32) -> HandlerResult {
        AvailableColumnsPanel_eventHandler(self, ev)
    }
}

/// Port of `static void AvailableColumnsPanel_delete(Object* object)` from
/// `AvailableColumnsPanel.c:32`: `Panel_done(&this->super); free(this);`.
/// Taking `this` by value consumes the panel; the embedded `super_`
/// [`Panel`] is handed to [`Panel_done`] (mirroring the C call graph), and
/// the non-owning `columns` back-pointer drops with the struct free.
pub fn AvailableColumnsPanel_delete(this: AvailableColumnsPanel) {
    let AvailableColumnsPanel { super_, columns } = this;
    Panel_done(super_);
    let _ = columns;
}

/// Port of `static void AvailableColumnsPanel_insert(AvailableColumnsPanel* this,
/// int at, int key)` from `AvailableColumnsPanel.c:38`.
///
/// Chooses the column name via `key >= ROW_DYNAMIC_FIELDS ?
/// DynamicColumn_name(key) : Process_fields[key].name` and inserts a
/// `ListItem_new(name, key)` at `at` into `this->columns` (the raw back-pointer
/// to the `ColumnsPanel`'s panel).
///
/// # Safety
///
/// `this.columns` must be the valid non-owning `Panel` pointer stored by
/// [`AvailableColumnsPanel_new`].
pub fn AvailableColumnsPanel_insert(this: &mut AvailableColumnsPanel, at: i32, key: i32) {
    let name: &str = if key >= ROW_DYNAMIC_FIELDS {
        DynamicColumn_name(key as u32)
            .expect("AvailableColumnsPanel_insert: DynamicColumn_name returned None")
    } else {
        Process_fields[key as usize].name
    };
    // C: Panel_insert(this->columns, at, (Object*) ListItem_new(name, key));
    // SAFETY: `columns` is the non-owning back-pointer stored at construction.
    let columns = unsafe { &mut *this.columns };
    Panel_insert(columns, at, Box::new(ListItem_new(name, key)));
}

/// Port of `static HandlerResult AvailableColumnsPanel_eventHandler(Panel* super,
/// int ch)` from `AvailableColumnsPanel.c:47`.
///
/// On Enter (`13`/`KEY_ENTER`), `KEY_RECLICK` or `F5`: reads the selected
/// [`ListItem`]'s `key`, inserts that column into `this->columns` at the
/// columns panel's selected index via [`AvailableColumnsPanel_insert`], moves
/// the columns selection down one, and refreshes it with [`ColumnsPanel_update`].
/// The default arm forwards a printable key to [`Panel_selectByTyping`].
///
/// Following the sibling panel port convention the C `Panel* super` upcast to
/// `AvailableColumnsPanel*` becomes the receiver `this: &mut AvailableColumnsPanel`.
pub fn AvailableColumnsPanel_eventHandler(
    this: &mut AvailableColumnsPanel,
    ch: i32,
) -> HandlerResult {
    const KEY_F5: i32 = KEY_F(5);

    let mut result = HandlerResult::IGNORED;

    match ch {
        13 | KEY_ENTER | KEY_RECLICK | KEY_F5 => {
            // const ListItem* selected = (ListItem*) Panel_getSelected(super);
            // if (!selected) break;
            let key = match Panel_getSelected(&this.super_) {
                None => return result,
                Some(obj) => {
                    let any: &dyn core::any::Any = obj;
                    any.downcast_ref::<ListItem>()
                        .expect("AvailableColumnsPanel_eventHandler: selected is not a ListItem")
                        .key
                }
            };

            // SAFETY: `columns` is the non-owning back-pointer stored at
            // construction; it outlives this panel.
            let at = Panel_getSelectedIndex(unsafe { &*this.columns });
            AvailableColumnsPanel_insert(this, at, key);
            Panel_setSelected(unsafe { &mut *this.columns }, at + 1);
            ColumnsPanel_update(unsafe { &mut *this.columns });
            result = HandlerResult::HANDLED;
        }
        _ => {
            // C: if (0 < ch && ch < 255 && isgraph((unsigned char)ch))
            //       result = Panel_selectByTyping(super, ch);
            if 0 < ch && ch < 255 && (ch as u8).is_ascii_graphic() {
                result = Panel_selectByTyping(&mut this.super_, ch);
            }
        }
    }

    result
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

/// Port of `static void AvailableColumnsPanel_addPlatformColumns(AvailableColumnsPanel* this)`
/// from `AvailableColumnsPanel.c:105`.
///
/// Loops `1..LAST_PROCESSFIELD`, and for each field with a description formats
/// `"<name> - <description>"` and `Panel_add`s a `ListItem_new(description, i)`.
/// C's `xSnprintf` into a fixed `char description[256]` becomes a heap
/// `format!`; the 256-byte truncation is a fixed-buffer artifact, not part of
/// the column-listing semantics.
pub fn AvailableColumnsPanel_addPlatformColumns(this: &mut AvailableColumnsPanel) {
    for i in 1..LAST_PROCESSFIELD {
        if let Some(desc) = Process_fields[i].description {
            let description = format!("{} - {}", Process_fields[i].name, desc);
            Panel_add(
                &mut this.super_,
                Box::new(ListItem_new(&description, i as i32)),
            );
        }
    }
}

/// Port of `static void AvailableColumnsPanel_addDynamicScreens(AvailableColumnsPanel* this,
/// const char* screen)` from `AvailableColumnsPanel.c:116`. Thin wrapper
/// delegating to [`Platform_addDynamicScreenAvailableColumns`] (a non-PCP
/// no-op); C passes `&this->super` (the embedded `Panel`), rendered here as
/// `&mut this.super_`.
pub fn AvailableColumnsPanel_addDynamicScreens(this: &mut AvailableColumnsPanel, screen: &str) {
    // C: Platform_addDynamicScreenAvailableColumns(&this->super, screen);
    Platform_addDynamicScreenAvailableColumns(&mut this.super_, screen);
}

/// Port of `void AvailableColumnsPanel_fill(AvailableColumnsPanel* this,
/// const char* dynamicScreen, Hashtable* dynamicColumns)` from
/// `AvailableColumnsPanel.c:120`.
///
/// [`Panel_prune`]s the panel, then — for a `dynamicScreen` — calls
/// [`AvailableColumnsPanel_addDynamicScreens`] (delegating to the non-PCP
/// no-op [`Platform_addDynamicScreenAvailableColumns`]); otherwise
/// [`AvailableColumnsPanel_addPlatformColumns`] then
/// [`AvailableColumnsPanel_addDynamicColumns`]. `dynamicScreen` /
/// `dynamicColumns` are `Option<&str>` / `Option<&Hashtable>` (the C
/// NULL-able `const char*` / non-null-in-the-else-branch `Hashtable*`).
pub fn AvailableColumnsPanel_fill(
    this: &mut AvailableColumnsPanel,
    dynamicScreen: Option<&str>,
    dynamicColumns: Option<&Hashtable>,
) {
    Panel_prune(&mut this.super_);
    if let Some(screen) = dynamicScreen {
        AvailableColumnsPanel_addDynamicScreens(this, screen);
    } else {
        AvailableColumnsPanel_addPlatformColumns(this);
        AvailableColumnsPanel_addDynamicColumns(
            this,
            dynamicColumns.expect(
                "AvailableColumnsPanel_fill: dynamicColumns is NULL in the non-screen branch",
            ),
        );
    }
}

/// Port of `AvailableColumnsPanel* AvailableColumnsPanel_new(Panel* columns,
/// Hashtable* dynamicColumns)` from `AvailableColumnsPanel.c:130`.
///
/// Builds a `1×1` [`Panel`] with the `AvailableColumnsFunctions`
/// [`FunctionBar`](crate::ported::functionbar::FunctionBar), sets the
/// "Available Columns" header, stores the `columns` back-pointer, and
/// populates the list via [`AvailableColumnsPanel_fill`]`(this, NULL,
/// dynamicColumns)`. The C `Class(ListItem)`/`owner` args to `Panel_init` type
/// the underlying `Vector`; the ported `Panel_new` drops them, matching every
/// sibling panel port.
pub fn AvailableColumnsPanel_new(
    columns: *mut Panel,
    dynamicColumns: &Hashtable,
) -> AvailableColumnsPanel {
    let fuBar = FunctionBar_new(Some(&AvailableColumnsFunctions[..]), None, None);
    let super_ = Panel_new(1, 1, 1, 1, Some(fuBar));

    let mut this = AvailableColumnsPanel { super_, columns };

    Panel_setHeader(&mut this.super_, "Available Columns");

    // C: this->columns = columns; (set above)
    //    AvailableColumnsPanel_fill(this, NULL, dynamicColumns);
    AvailableColumnsPanel_fill(&mut this, None, Some(dynamicColumns));

    this
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
            columns: core::ptr::null_mut(),
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
        let obj: &dyn core::any::Any = this.super_.items[i].object();
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
