//! Partial port of `OptionItem.c` — htop's Setup-screen option widgets.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`, and the
//! struct fields keep their C spelling too), so `non_snake_case` is
//! allowed for the whole module.
//!
//! Ported here: the pure value accessors and edit-buffer state machine
//! for [`CheckItem`] and [`NumberItem`] — `CheckItem_get`/`set`/`toggle`
//! and the `NumberItem_*` get/decrease/increase/toggle/editing family.
//! These are self-contained arithmetic + string logic with no dependency
//! on unported substrate.
//!
//! Also ported: `TextItem_display`, `CheckItem_display`, and
//! `NumberItem_display` — the option-row renderers — plus the [`Object`]
//! vtable wiring (`impl Object for {TextItem, CheckItem, NumberItem}`) that
//! dispatches `display` faithfully. They build the row through the real
//! [`RichString`]/[`ColorElements`] substrate (checkbox glyphs, spacing, and
//! `CRT_colors[CHECK_BOX/CHECK_MARK/CHECK_TEXT/HELP_BOLD]` exactly as htop).
//! The label (C `OptionItem.super.text`) is modeled as a plain `text: String`
//! field on each struct — pure data, no object substrate needed.
//!
//! Also ported: the direct-value constructors `TextItem_new`,
//! `CheckItem_newByVal`, and `NumberItem_newByVal`. The C `AllocThis`/
//! `xStrdup` + `Object`-super wiring becomes a struct literal plus an owned
//! `text` copy (the vtable is supplied by `impl Object`), so these are
//! faithful ports of the `ref == NULL` construction path — including
//! `NumberItem_newByVal`'s `assert(min <= max)` and `CLAMP(value, min, max)`.
//!
//! Also ported: `CheckItem_newByRef` / `NumberItem_newByRef` — the
//! external-cell constructors. The C `CheckItem`/`NumberItem` is a tagged
//! union of a direct value and a pointer into an external cell
//! (`bool* ref` / `int* ref`); the `*_newByRef` constructors set that
//! pointer. This is modeled faithfully with a raw pointer field
//! (`ref_: *mut bool` / `*mut c_int`) on each struct: `null` is the C
//! `ref == NULL` (direct-value) case, non-`null` the external-cell case.
//! Every accessor ports BOTH C branches — `if (this->ref) *ref … else
//! value …` — dereferencing `ref_` inside `unsafe` (the external cell is a
//! Settings field, which outlives the option item; the setup screen that
//! builds these items borrows the live `Settings`).
//!
//! `OptionItem_delete` frees through the `Object` cast; its safe-Rust
//! analog is `Drop` (the boxed `dyn Object` dropping runs the concrete
//! subtype's free), so it is a by-value consume, not a `todo!()`.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(dead_code)]

use core::any::Any;
use core::ffi::c_int;

use crate::ported::crt::{ColorElements, ColorScheme};
use crate::ported::object::{Object, ObjectClass, Object_class};
use crate::ported::richstring::{
    RichString, RichString_appendAscii, RichString_appendWide, RichString_appendnAscii,
    RichString_writeAscii,
};

/// Maximum number of characters in a [`NumberItem`] edit buffer.
/// Port of `#define NUMBERITEM_EDIT_MAX 10` from `OptionItem.h:14`.
pub const NUMBERITEM_EDIT_MAX: usize = 10;

/// Port of `enum OptionItemType` (`OptionItem.h:16`) — the `.kind`
/// discriminant carried on `OptionItemClass` distinguishing the three
/// option-row subtypes. The C enumerators are `OPTION_ITEM_TEXT` (0),
/// `OPTION_ITEM_CHECK` (1), `OPTION_ITEM_NUMBER` (2). Preserved verbatim
/// (matching the spec name-for-name).
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OptionItemType {
    OPTION_ITEM_TEXT = 0,
    OPTION_ITEM_CHECK = 1,
    OPTION_ITEM_NUMBER = 2,
}

/// Port of the `OptionItem_kind(this_)` macro (`OptionItem.h:29`):
/// `As_OptionItem(this_)->kind`. In C the kind is a field on the item's
/// `OptionItemClass` vtable (`TextItem_class.kind == OPTION_ITEM_TEXT`,
/// etc.); the ported subtypes are distinct Rust structs, so the vtable-field
/// read is the `Any` downcast that recovers the concrete type. Panics on a
/// non-OptionItem object, matching the C hard cast (UB on a wrong type).
pub fn OptionItem_kind(this: &dyn Object) -> OptionItemType {
    let any: &dyn Any = this;
    if any.is::<TextItem>() {
        OptionItemType::OPTION_ITEM_TEXT
    } else if any.is::<CheckItem>() {
        OptionItemType::OPTION_ITEM_CHECK
    } else if any.is::<NumberItem>() {
        OptionItemType::OPTION_ITEM_NUMBER
    } else {
        panic!("OptionItem_kind: object is not a TextItem/CheckItem/NumberItem");
    }
}

/// Model of C `TextItem` (`OptionItem.h:38`). `TextItem_display` renders
/// `this->super.text`, modeled here as the `text` field (C's own
/// `TextItem.text` shadow field is unused/dead in the C source).
pub struct TextItem {
    pub text: String,
}

/// Model of C `CheckItem` (`OptionItem.h:43`). The C struct is a tagged
/// union of a direct value (`bool value`) and an external cell
/// (`bool* ref`): when `ref` is non-`NULL` every accessor reads/writes
/// `*ref`, else it uses `value`. Both are modeled here — `value` is the
/// direct field and `ref_` is the raw `*mut bool` (`ref` is a Rust keyword)
/// that [`CheckItem_newByRef`] sets to point into an external `bool` (a
/// Settings field cell). `ref_` is `null` for a `_newByVal` item. `text`
/// models the C `OptionItem.super.text` label rendered by
/// [`CheckItem_display`].
pub struct CheckItem {
    pub value: bool,
    /// C `bool* ref` (`OptionItem.h:45`) — a raw pointer to an external
    /// `bool` cell (a Settings field), or `null` for a direct-value item.
    pub ref_: *mut bool,
    pub text: String,
}

/// Model of C `NumberItem` (`OptionItem.h:50`). Like [`CheckItem`] it is a
/// tagged union of a direct `int value` and an external cell `int* ref`:
/// every accessor reads/writes `*ref` when `ref` is non-`NULL`, else
/// `value`. Both are modeled — `value` is the direct field, `ref_` the raw
/// `*mut c_int` ([`NumberItem_newByRef`] points it into a Settings field
/// cell; `null` for a `_newByVal` item). The C `char
/// editBuffer[NUMBERITEM_EDIT_MAX + 1]` + `int editLen` pair is modeled as a
/// single `String` whose length is the C `editLen` (the two are always kept
/// in lockstep in the C source).
pub struct NumberItem {
    pub value: i32,
    /// C `int* ref` (`OptionItem.h:52`) — a raw pointer to an external
    /// `int` cell (a Settings field), or `null` for a direct-value item.
    pub ref_: *mut c_int,
    pub scale: i32,
    pub min: i32,
    pub max: i32,
    pub editing: bool,
    pub editBuffer: String,
    pub savedValue: i32,
    /// C `OptionItem.super.text` — the label rendered by [`NumberItem_display`].
    pub text: String,
}

/// Port of `static void OptionItem_delete(Object* cast)` from
/// `OptionItem.c:23`: `free(this->text); free(this);`. This is the shared
/// `.delete` vtable slot for all three subtypes (`TextItem`/`CheckItem`/
/// `NumberItem` — each `impl Object` here). Taking the object by its boxed
/// `dyn Object` cast (the safe-Rust analog of the C `Object*`) consumes it;
/// dropping the box runs the concrete subtype's `Drop`, freeing its owned
/// `text` `String` and the struct — the whole C free routine.
pub fn OptionItem_delete(this: Box<dyn Object>) {
    let _ = this;
}

/// Port of `OptionItem.c:31` (`static void TextItem_display`). Appends the
/// label `this->super.text` in `CRT_colors[HELP_BOLD]`
/// (`HELP_BOLD.packed(active scheme)`).
pub fn TextItem_display(this: &TextItem, out: &mut RichString) {
    let help_bold = ColorElements::HELP_BOLD.packed(ColorScheme::active());
    RichString_appendWide(out, help_bold, this.text.as_bytes());
}

/// Port of `OptionItem.c:38` (`static void CheckItem_display`). Renders the
/// checkbox row `"[x]    <label>"` / `"[ ]    <label>"`: `"["` in
/// `CRT_colors[CHECK_BOX]`, the `x`/space mark in `CRT_colors[CHECK_MARK]`,
/// the literal `"]    "` (bracket + four spaces) in `CRT_colors[CHECK_BOX]`,
/// then the label `this->super.text` in `CRT_colors[CHECK_TEXT]`.
pub fn CheckItem_display(this: &CheckItem, out: &mut RichString) {
    let scheme = ColorScheme::active();
    let check_box = ColorElements::CHECK_BOX.packed(scheme);
    let check_mark = ColorElements::CHECK_MARK.packed(scheme);
    let check_text = ColorElements::CHECK_TEXT.packed(scheme);

    RichString_writeAscii(out, check_box, b"[");
    if CheckItem_get(this) {
        RichString_appendAscii(out, check_mark, b"x");
    } else {
        RichString_appendAscii(out, check_mark, b" ");
    }
    RichString_appendAscii(out, check_box, b"]    ");
    RichString_appendWide(out, check_text, this.text.as_bytes());
}

/// Port of `OptionItem.c:52` (`static void NumberItem_display`). Renders the
/// number row `"[<value>]"` followed by right-padding to a 5-column field and
/// the label. The bracketed value is drawn in `CRT_colors[CHECK_MARK]` and is
/// (in C's branch order): the raw `editBuffer` while `editing`; else
/// `%.*f` of `10^scale * value` with `-scale` decimals when `scale < 0`;
/// else the truncated integer `10^scale * value` when `scale > 0`; else the
/// plain integer value. `written` is the character count of that field; the
/// C `for (i = written; i < 5; i++)` loop pads with `CRT_colors[CHECK_BOX]`
/// spaces (no padding once `written >= 5`). The brackets are
/// `CRT_colors[CHECK_BOX]`; the label `this->super.text` is
/// `CRT_colors[CHECK_TEXT]`.
pub fn NumberItem_display(this: &NumberItem, out: &mut RichString) {
    let scheme = ColorScheme::active();
    let check_box = ColorElements::CHECK_BOX.packed(scheme);
    let check_mark = ColorElements::CHECK_MARK.packed(scheme);
    let check_text = ColorElements::CHECK_TEXT.packed(scheme);

    RichString_writeAscii(out, check_box, b"[");
    let written: usize;
    if this.editing {
        // C: written = this->editLen; append editBuffer[0..editLen].
        written = this.editBuffer.len();
        RichString_appendnAscii(out, check_mark, this.editBuffer.as_bytes(), written);
    } else if this.scale < 0 {
        let buffer = format!(
            "{:.*}",
            (-this.scale) as usize,
            10f64.powi(this.scale) * NumberItem_get(this) as f64
        );
        written = buffer.len();
        RichString_appendnAscii(out, check_mark, buffer.as_bytes(), written);
    } else if this.scale > 0 {
        let buffer = format!(
            "{}",
            (10f64.powi(this.scale) * NumberItem_get(this) as f64) as i32
        );
        written = buffer.len();
        RichString_appendnAscii(out, check_mark, buffer.as_bytes(), written);
    } else {
        let buffer = format!("{}", NumberItem_get(this));
        written = buffer.len();
        RichString_appendnAscii(out, check_mark, buffer.as_bytes(), written);
    }
    RichString_appendAscii(out, check_box, b"]");
    // C: for (int i = written; i < 5; i++) — empty range once written >= 5.
    for _ in written..5 {
        RichString_appendAscii(out, check_box, b" ");
    }
    RichString_appendWide(out, check_text, this.text.as_bytes());
}

/// Port of `const OptionItemClass OptionItem_class` (`OptionItem.c:78`):
/// `.super.extends = Class(Object)`. Declared `static` for stable identity
/// (see [`Object_isA`]). Only the `extends` link is modeled here; the C
/// `.kind` field lives on `OptionItemClass`, which is not needed for display
/// dispatch.
static OptionItem_class: ObjectClass = ObjectClass {
    extends: Some(&Object_class),
};

/// Port of `const OptionItemClass TextItem_class` (`OptionItem.c:86`):
/// `.super.extends = Class(OptionItem)`, `.super.display = TextItem_display`.
static TextItem_class: ObjectClass = ObjectClass {
    extends: Some(&OptionItem_class),
};

/// Port of `const OptionItemClass CheckItem_class` (`OptionItem.c:96`):
/// `.super.extends = Class(OptionItem)`, `.super.display = CheckItem_display`.
static CheckItem_class: ObjectClass = ObjectClass {
    extends: Some(&OptionItem_class),
};

/// Port of `const OptionItemClass NumberItem_class` (`OptionItem.c:106`):
/// `.super.extends = Class(OptionItem)`, `.super.display = NumberItem_display`.
static NumberItem_class: ObjectClass = ObjectClass {
    extends: Some(&OptionItem_class),
};

impl Object for TextItem {
    fn klass(&self) -> &'static ObjectClass {
        &TextItem_class
    }
    fn display(&self, out: &mut RichString) {
        TextItem_display(self, out);
    }
}

impl Object for CheckItem {
    fn klass(&self) -> &'static ObjectClass {
        &CheckItem_class
    }
    fn display(&self, out: &mut RichString) {
        CheckItem_display(self, out);
    }
}

impl Object for NumberItem {
    fn klass(&self) -> &'static ObjectClass {
        &NumberItem_class
    }
    fn display(&self, out: &mut RichString) {
        NumberItem_display(self, out);
    }
}

/// Port of `TextItem_new` from `OptionItem.c:115`. Allocates a `TextItem`
/// and copies the label (C `AllocThis(TextItem)` + `xStrdup(text)`). The C
/// `AllocThis` also zero-inits and wires the `Object` vtable, which is
/// handled here by `impl Object for TextItem`; the only data field is
/// `text`. Returns the value (C returns a heap pointer).
pub fn TextItem_new(text: &str) -> TextItem {
    TextItem {
        text: text.to_string(),
    }
}

/// Port of `CheckItem* CheckItem_newByRef(const char* text, bool* ref)` from
/// `OptionItem.c:121`. Builds an external-cell `CheckItem`: C sets
/// `value = false` and `ref = ref` (a non-`NULL` pointer to an external
/// `bool`, a Settings field). The C `AllocThis(CheckItem)` + `xStrdup(text)`
/// become the struct literal + an owned `text` copy; `ref_` carries the raw
/// `*mut bool` (see the struct docs). The caller guarantees `ref` outlives
/// the item.
pub fn CheckItem_newByRef(text: &str, ref_: *mut bool) -> CheckItem {
    CheckItem {
        value: false,
        ref_,
        text: text.to_string(),
    }
}

/// Port of `CheckItem_newByVal` from `OptionItem.c:129`. Builds a
/// direct-value `CheckItem`: C sets `value = value` and `ref = NULL` (here
/// `ref_` is a null pointer). The C `AllocThis(CheckItem)` + `xStrdup(text)`
/// become the struct literal + an owned `text` copy.
pub fn CheckItem_newByVal(text: &str, value: bool) -> CheckItem {
    CheckItem {
        value,
        ref_: core::ptr::null_mut(),
        text: text.to_string(),
    }
}

/// Port of `CheckItem_get` from `OptionItem.c:137`. Returns `*ref` when the
/// item aliases an external cell, else the direct `value`.
pub fn CheckItem_get(this: &CheckItem) -> bool {
    if !this.ref_.is_null() {
        // SAFETY: `ref_` is a non-null pointer into an external `bool`
        // (a Settings field) set by `CheckItem_newByRef`; that cell
        // outlives the item (see module docs).
        unsafe { *this.ref_ }
    } else {
        this.value
    }
}

/// Port of `CheckItem_set` from `OptionItem.c:145`. Writes `*ref` when the
/// item aliases an external cell, else the direct `value`.
pub fn CheckItem_set(this: &mut CheckItem, value: bool) {
    if !this.ref_.is_null() {
        // SAFETY: see `CheckItem_get`.
        unsafe { *this.ref_ = value };
    } else {
        this.value = value;
    }
}

/// Port of `CheckItem_toggle` from `OptionItem.c:153`. Flips `*ref` when the
/// item aliases an external cell, else the direct `value`.
pub fn CheckItem_toggle(this: &mut CheckItem) {
    if !this.ref_.is_null() {
        // SAFETY: see `CheckItem_get`.
        unsafe { *this.ref_ = !*this.ref_ };
    } else {
        this.value = !this.value;
    }
}

/// Port of `NumberItem* NumberItem_newByRef(const char* text, int* ref, int
/// scale, int min, int max)` from `OptionItem.c:161`. Builds an external-cell
/// `NumberItem`: C sets `value = 0` and `ref = ref` (a non-`NULL` pointer to
/// an external `int`, a Settings field), preserves the `assert(min <= max)`
/// precondition, and zero-inits the edit-buffer fields (`editing = false`,
/// empty buffer, `savedValue = 0`). The C `AllocThis(NumberItem)` +
/// `xStrdup(text)` become the struct literal + an owned `text` copy; `ref_`
/// carries the raw `*mut c_int` (see the struct docs). Unlike
/// [`NumberItem_newByVal`], the initial value is NOT clamped (C leaves the
/// external cell untouched at construction). The caller guarantees `ref`
/// outlives the item.
pub fn NumberItem_newByRef(
    text: &str,
    ref_: *mut c_int,
    scale: i32,
    min: i32,
    max: i32,
) -> NumberItem {
    assert!(min <= max);
    NumberItem {
        value: 0,
        ref_,
        scale,
        min,
        max,
        editing: false,
        editBuffer: String::new(),
        savedValue: 0,
        text: text.to_string(),
    }
}

/// Port of `NumberItem_newByVal` from `OptionItem.c:178`. Builds a
/// direct-value `NumberItem`: C sets `ref = NULL` — the `ref == NULL` case
/// this struct models — and clamps the initial value to `[min, max]`
/// (C `CLAMP(value, min, max)`). The C `assert(min <= max)` precondition
/// is preserved. `AllocThis(NumberItem)` + `xStrdup(text)` become the
/// struct literal + an owned `text` copy; the edit-buffer fields are
/// zero-initialized exactly as in C (`editing = false`, empty buffer,
/// `savedValue = 0`).
pub fn NumberItem_newByVal(text: &str, value: i32, scale: i32, min: i32, max: i32) -> NumberItem {
    assert!(min <= max);
    // CLAMP(value, min, max)
    let value = if value > max {
        max
    } else if value < min {
        min
    } else {
        value
    };
    NumberItem {
        value,
        ref_: core::ptr::null_mut(),
        scale,
        min,
        max,
        editing: false,
        editBuffer: String::new(),
        savedValue: 0,
        text: text.to_string(),
    }
}

/// Port of `NumberItem_get` from `OptionItem.c:195`. Returns `*ref` when the
/// item aliases an external cell, else the direct `value`.
pub fn NumberItem_get(this: &NumberItem) -> i32 {
    if !this.ref_.is_null() {
        // SAFETY: `ref_` is a non-null pointer into an external `int`
        // (a Settings field) set by `NumberItem_newByRef`; that cell
        // outlives the item (see module docs).
        unsafe { *this.ref_ }
    } else {
        this.value
    }
}

/// Port of `NumberItem_decrease` from `OptionItem.c:203`. Decrements and
/// re-clamps to `[min, max]` (C `CLAMP`), on `*ref` when the item aliases an
/// external cell, else on the direct `value`.
pub fn NumberItem_decrease(this: &mut NumberItem) {
    // CLAMP(x, low, high) == (x > high) ? high : (x < low ? low : x).
    if !this.ref_.is_null() {
        // SAFETY: see `NumberItem_get`.
        unsafe {
            let v = *this.ref_ - 1;
            *this.ref_ = if v > this.max {
                this.max
            } else if v < this.min {
                this.min
            } else {
                v
            };
        }
    } else {
        let v = this.value - 1;
        this.value = if v > this.max {
            this.max
        } else if v < this.min {
            this.min
        } else {
            v
        };
    }
}

/// Port of `NumberItem_increase` from `OptionItem.c:211`. Increments and
/// re-clamps to `[min, max]` (C `CLAMP`), on `*ref` when the item aliases an
/// external cell, else on the direct `value`.
pub fn NumberItem_increase(this: &mut NumberItem) {
    // CLAMP(x, low, high) == (x > high) ? high : (x < low ? low : x).
    if !this.ref_.is_null() {
        // SAFETY: see `NumberItem_get`.
        unsafe {
            let v = *this.ref_ + 1;
            *this.ref_ = if v > this.max {
                this.max
            } else if v < this.min {
                this.min
            } else {
                v
            };
        }
    } else {
        let v = this.value + 1;
        this.value = if v > this.max {
            this.max
        } else if v < this.min {
            this.min
        } else {
            v
        };
    }
}

/// Port of `NumberItem_toggle` from `OptionItem.c:219`. Steps by one,
/// wrapping back to `min` once at or above `max`, on `*ref` when the item
/// aliases an external cell, else on the direct `value`.
pub fn NumberItem_toggle(this: &mut NumberItem) {
    if !this.ref_.is_null() {
        // SAFETY: see `NumberItem_get`.
        unsafe {
            if *this.ref_ >= this.max {
                *this.ref_ = this.min;
            } else {
                *this.ref_ += 1;
            }
        }
    } else if this.value >= this.max {
        this.value = this.min;
    } else {
        this.value += 1;
    }
}

/// Port of `NumberItem_startEditing` from `OptionItem.c:233`. Saves the
/// current value, enters editing mode, and empties the edit buffer
/// (C `editLen = 0; editBuffer[0] = '\0'`).
pub fn NumberItem_startEditing(this: &mut NumberItem) {
    this.savedValue = NumberItem_get(this);
    this.editing = true;
    this.editBuffer.clear();
}

/// Port of `NumberItem_startEditingFromValue` from `OptionItem.c:240`.
/// Seeds the edit buffer with the current value, formatted with decimal
/// places when `scale < 0` (`%.*f` on `10^scale * savedValue`) or as a
/// plain integer otherwise, truncated to `NUMBERITEM_EDIT_MAX` bytes
/// (C `MINIMUM(len, NUMBERITEM_EDIT_MAX)` + `memcpy`). The formatted
/// text is ASCII, so byte truncation matches the C `memcpy` exactly.
pub fn NumberItem_startEditingFromValue(this: &mut NumberItem) {
    this.savedValue = NumberItem_get(this);
    this.editing = true;
    let tmp = if this.scale < 0 {
        format!(
            "{:.*}",
            (-this.scale) as usize,
            10f64.powi(this.scale) * this.savedValue as f64
        )
    } else {
        format!("{}", this.savedValue)
    };
    let edit_len = tmp.len().min(NUMBERITEM_EDIT_MAX);
    this.editBuffer = tmp[..edit_len].to_string();
}

/// Port of `NumberItem_cancelEditing` from `OptionItem.c:256`. Leaves
/// editing mode and empties the edit buffer.
pub fn NumberItem_cancelEditing(this: &mut NumberItem) {
    this.editing = false;
    this.editBuffer.clear();
}

/// Port of `NumberItem_applyEditing` from `OptionItem.c:262`. Leaves
/// editing mode, parses the edit buffer, clamps to `[min, max]`, and
/// commits. Returns `false` (leaving the value unchanged) when the
/// buffer is empty or holds no parseable numeric prefix.
///
/// The C `strtol`/`strtod` calls parse a leading numeric prefix and set
/// `endptr`; `endptr == editBuffer` (no characters consumed) is the
/// failure signal. That leading-prefix semantics is reproduced inline:
/// optional leading whitespace, optional sign, then digits (plus a
/// fractional part for the `scale < 0` path). `strtod`'s exponent /
/// hex / inf / nan forms are unreachable — the edit buffer only ever
/// contains `[0-9.]` from [`NumberItem_addChar`] or a `%d`/`%.*f`
/// seed from [`NumberItem_startEditingFromValue`] — so they are omitted.
/// Commits to `*ref` when the item aliases an external cell, else to the
/// direct `value`.
pub fn NumberItem_applyEditing(this: &mut NumberItem) -> bool {
    this.editing = false;
    if this.editBuffer.is_empty() {
        return false;
    }
    let bytes = this.editBuffer.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    let conv_start = i;
    if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
        i += 1;
    }

    let new_value: i32;
    if this.scale < 0 {
        // strtod: integer digits, optional '.', fractional digits.
        let mut has_digit = false;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
            has_digit = true;
        }
        if i < bytes.len() && bytes[i] == b'.' {
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
                has_digit = true;
            }
        }
        if !has_digit {
            // endptr == editBuffer: no conversion.
            this.editBuffer.clear();
            return false;
        }
        let display_value: f64 = this.editBuffer[conv_start..i].parse().unwrap_or(0.0);
        new_value = (display_value / 10f64.powi(this.scale)).round() as i32;
    } else {
        // strtol base 10: digits only.
        let digit_start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i == digit_start {
            // endptr == editBuffer: no conversion.
            this.editBuffer.clear();
            return false;
        }
        let parsed: i64 = this.editBuffer[conv_start..i].parse().unwrap_or(0);
        new_value = parsed as i32;
    }

    // CLAMP(newValue, min, max), then commit to *ref or the direct value.
    let clamped = if new_value > this.max {
        this.max
    } else if new_value < this.min {
        this.min
    } else {
        new_value
    };
    if !this.ref_.is_null() {
        // SAFETY: see `NumberItem_get`.
        unsafe { *this.ref_ = clamped };
    } else {
        this.value = clamped;
    }
    this.editBuffer.clear();
    true
}

/// Port of `NumberItem_addChar` from `OptionItem.c:298`. Appends one
/// character to the edit buffer, returning whether it was accepted.
/// `,` is normalized to `.`; a `.` is accepted only when `scale < 0`
/// and no `.` is already present; any other non-digit is rejected; and
/// the buffer is capped at `NUMBERITEM_EDIT_MAX` characters.
pub fn NumberItem_addChar(this: &mut NumberItem, c: char) -> bool {
    let mut c = c;
    if c == ',' {
        c = '.';
    }
    if c == '.' {
        if this.scale >= 0 {
            return false;
        }
        if this.editBuffer.contains('.') {
            return false;
        }
    } else if !c.is_ascii_digit() {
        return false;
    }
    if this.editBuffer.len() >= NUMBERITEM_EDIT_MAX {
        return false;
    }
    this.editBuffer.push(c);
    true
}

/// Port of `NumberItem_deleteChar` from `OptionItem.c:320`. Removes the
/// last character from the edit buffer, if any (C decrements `editLen`
/// and re-terminates).
pub fn NumberItem_deleteChar(this: &mut NumberItem) {
    this.editBuffer.pop();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn number_item(value: i32, scale: i32, min: i32, max: i32) -> NumberItem {
        NumberItem {
            value,
            ref_: core::ptr::null_mut(),
            scale,
            min,
            max,
            editing: false,
            editBuffer: String::new(),
            savedValue: 0,
            text: String::new(),
        }
    }

    /// Visible characters of the valid `[0, chlen)` range.
    fn rendered(rs: &RichString) -> String {
        rs.chptr
            .iter()
            .take(rs.chlen as usize)
            .map(|c| c.chars)
            .collect()
    }

    // ── external-cell (`_newByRef`) accessors ────────────────────────
    //
    // These exercise the `if (this->ref)` branch of every accessor: the
    // item aliases an external cell (a `bool`/`int` standing in for a
    // Settings field), and get/set/toggle/increase/decrease/applyEditing
    // must read and write *that* cell, never the item's own `value`.

    #[test]
    fn check_item_by_ref_reads_and_writes_external_cell() {
        let mut cell: bool = false;
        let mut it = CheckItem_newByRef("Follow", &mut cell as *mut bool);
        // value stays the newByRef default; every op goes through the cell.
        assert!(!it.value);
        assert!(!CheckItem_get(&it));

        CheckItem_set(&mut it, true);
        assert!(cell, "set writes the external cell");
        assert!(CheckItem_get(&it));
        assert!(!it.value, "the item's own value is never touched");

        CheckItem_toggle(&mut it);
        assert!(!cell, "toggle flips the external cell");
        assert!(!CheckItem_get(&it));

        // A direct write to the external cell is observed by the item. The
        // `&& cell` reads the binding directly (the item's read goes through
        // the raw pointer, which the unused-assignment lint cannot see).
        cell = true;
        assert!(CheckItem_get(&it) && cell);
    }

    #[test]
    fn number_item_by_ref_reads_and_writes_external_cell() {
        let mut cell: c_int = 5;
        let mut it = NumberItem_newByRef("Delay", &mut cell as *mut c_int, 0, 0, 10);
        // newByRef sets value = 0 and does NOT clamp the cell.
        assert_eq!(it.value, 0);
        assert_eq!(NumberItem_get(&it), 5);

        NumberItem_increase(&mut it);
        assert_eq!(cell, 6);
        NumberItem_decrease(&mut it);
        assert_eq!(cell, 5);
        assert_eq!(it.value, 0, "the item's own value is never touched");

        // Increase clamps the external cell at max.
        cell = 10;
        NumberItem_increase(&mut it);
        assert_eq!(cell, 10);
        // Decrease clamps at min.
        cell = 0;
        NumberItem_decrease(&mut it);
        assert_eq!(cell, 0);

        // Toggle wraps the external cell at max.
        cell = 10;
        NumberItem_toggle(&mut it);
        assert_eq!(cell, 0);
        NumberItem_toggle(&mut it);
        assert_eq!(cell, 1);
    }

    #[test]
    fn number_item_by_ref_apply_editing_commits_to_external_cell() {
        let mut cell: c_int = 0;
        let mut it = NumberItem_newByRef("N", &mut cell as *mut c_int, 0, 0, 50);
        it.editBuffer = "27".to_string();
        assert!(NumberItem_applyEditing(&mut it));
        assert_eq!(cell, 27, "committed to the external cell");
        assert_eq!(it.value, 0, "not the item's own value");

        // Over-max input clamps into the external cell.
        it.editBuffer = "999".to_string();
        assert!(NumberItem_applyEditing(&mut it));
        assert_eq!(cell, 50);
    }

    #[test]
    fn check_item_get_set_toggle() {
        let mut it = CheckItem {
            value: false,
            ref_: core::ptr::null_mut(),
            text: String::new(),
        };
        assert!(!CheckItem_get(&it));
        CheckItem_set(&mut it, true);
        assert!(CheckItem_get(&it));
        CheckItem_toggle(&mut it);
        assert!(!CheckItem_get(&it));
        CheckItem_toggle(&mut it);
        assert!(CheckItem_get(&it));
    }

    // ── display renderers (chars + attrs pinned) ─────────────────────

    /// The masked `CRT_colors[element]` an ASCII/wide write path stores.
    fn attr_of(el: ColorElements) -> i32 {
        el.packed(ColorScheme::active()) & 0xffffff
    }

    #[test]
    fn text_item_display_renders_label_in_help_bold() {
        let it = TextItem {
            text: "General".to_string(),
        };
        let mut rs = RichString::new();
        TextItem_display(&it, &mut rs);
        assert_eq!(rendered(&rs), "General");
        for i in 0..rs.chlen as usize {
            assert_eq!(
                rs.chptr[i].attr,
                attr_of(ColorElements::HELP_BOLD),
                "attr at {i}"
            );
        }
    }

    #[test]
    fn check_item_display_checked_and_unchecked_glyphs() {
        let checked = CheckItem {
            value: true,
            ref_: core::ptr::null_mut(),
            text: "Tree view".to_string(),
        };
        let mut rs = RichString::new();
        CheckItem_display(&checked, &mut rs);
        assert_eq!(rendered(&rs), "[x]    Tree view");

        let unchecked = CheckItem {
            value: false,
            ref_: core::ptr::null_mut(),
            text: "Tree view".to_string(),
        };
        let mut rs2 = RichString::new();
        CheckItem_display(&unchecked, &mut rs2);
        assert_eq!(rendered(&rs2), "[ ]    Tree view");
    }

    #[test]
    fn check_item_display_attrs_per_cell() {
        let it = CheckItem {
            value: true,
            ref_: core::ptr::null_mut(),
            text: "ab".to_string(),
        };
        let mut rs = RichString::new();
        CheckItem_display(&it, &mut rs);
        // "[x]    ab" -> idx0 '[' box, idx1 'x' mark, idx2..=6 "]    " box,
        // idx7.. "ab" text.
        let box_c = attr_of(ColorElements::CHECK_BOX);
        let mark_c = attr_of(ColorElements::CHECK_MARK);
        let text_c = attr_of(ColorElements::CHECK_TEXT);
        assert_eq!(rendered(&rs), "[x]    ab");
        assert_eq!(rs.chptr[0].attr, box_c);
        assert_eq!(rs.chptr[1].attr, mark_c);
        for i in 2..=6 {
            assert_eq!(rs.chptr[i].attr, box_c, "bracket/space attr at {i}");
        }
        assert_eq!(rs.chptr[7].attr, text_c);
        assert_eq!(rs.chptr[8].attr, text_c);
    }

    #[test]
    fn number_item_display_integer_pads_to_five() {
        // value 42, scale 0 -> "[42]" + 3 pad spaces + label.
        let mut it = number_item(42, 0, 0, 1000);
        it.text = "Delay".to_string();
        let mut rs = RichString::new();
        NumberItem_display(&it, &mut rs);
        assert_eq!(rendered(&rs), "[42]   Delay");
        // '4','2' are the CHECK_MARK value cells.
        assert_eq!(rs.chptr[1].attr, attr_of(ColorElements::CHECK_MARK));
        assert_eq!(rs.chptr[2].attr, attr_of(ColorElements::CHECK_MARK));
        // trailing pad spaces are CHECK_BOX.
        assert_eq!(rs.chptr[4].attr, attr_of(ColorElements::CHECK_BOX));
        // label is CHECK_TEXT.
        assert_eq!(rs.chptr[7].attr, attr_of(ColorElements::CHECK_TEXT));
    }

    #[test]
    fn number_item_display_no_padding_when_field_at_least_five() {
        // "12345" is 5 chars -> for-loop range 5..5 is empty, no pad spaces.
        let mut it = number_item(12345, 0, 0, 100000);
        it.text = "N".to_string();
        let mut rs = RichString::new();
        NumberItem_display(&it, &mut rs);
        assert_eq!(rendered(&rs), "[12345]N");
    }

    #[test]
    fn number_item_display_scaled_decimal() {
        // scale -2 -> 10^-2 * 150 = 1.5 formatted "%.2f" = "1.50" (4 chars),
        // one pad space to reach the 5-field.
        let mut it = number_item(150, -2, 0, 100000);
        it.text = "Pct".to_string();
        let mut rs = RichString::new();
        NumberItem_display(&it, &mut rs);
        assert_eq!(rendered(&rs), "[1.50] Pct");
    }

    #[test]
    fn number_item_display_positive_scale_multiplies() {
        // scale 1 -> (int)(10 * 3) = 30 -> "[30]" + 3 pad + label.
        let mut it = number_item(3, 1, 0, 1000);
        it.text = "K".to_string();
        let mut rs = RichString::new();
        NumberItem_display(&it, &mut rs);
        assert_eq!(rendered(&rs), "[30]   K");
    }

    #[test]
    fn number_item_display_editing_uses_edit_buffer() {
        let mut it = number_item(999, 0, 0, 100000);
        it.editing = true;
        it.editBuffer = "7".to_string();
        it.text = "X".to_string();
        let mut rs = RichString::new();
        NumberItem_display(&it, &mut rs);
        // editBuffer "7" is 1 char -> 4 pad spaces.
        assert_eq!(rendered(&rs), "[7]    X");
        assert_eq!(rs.chptr[1].attr, attr_of(ColorElements::CHECK_MARK));
    }

    #[test]
    fn object_display_dispatches_for_each_kind() {
        let t = TextItem {
            text: "T".to_string(),
        };
        let mut rs = RichString::new();
        Object::display(&t, &mut rs);
        assert_eq!(rendered(&rs), "T");

        let c = CheckItem {
            value: false,
            ref_: core::ptr::null_mut(),
            text: "C".to_string(),
        };
        let mut rs2 = RichString::new();
        Object::display(&c, &mut rs2);
        assert_eq!(rendered(&rs2), "[ ]    C");

        let mut n = number_item(5, 0, 0, 100);
        n.text = "N".to_string();
        let mut rs3 = RichString::new();
        Object::display(&n, &mut rs3);
        assert_eq!(rendered(&rs3), "[5]    N");
    }

    #[test]
    fn number_item_new_by_val_clamps_initial_value() {
        // value above max clamps down to max (C CLAMP)
        let it = NumberItem_newByVal("Delay", 999, 0, 0, 50);
        assert_eq!(it.value, 50);
        assert_eq!(it.scale, 0);
        assert_eq!(it.min, 0);
        assert_eq!(it.max, 50);
        assert!(!it.editing);
        assert_eq!(it.savedValue, 0);
        assert!(it.editBuffer.is_empty());
        assert_eq!(it.text, "Delay");
        // value below min clamps up to min
        let lo = NumberItem_newByVal("N", -5, -2, 3, 100);
        assert_eq!(lo.value, 3);
        assert_eq!(lo.scale, -2);
        // in-range value passes through
        let mid = NumberItem_newByVal("M", 20, 0, 0, 50);
        assert_eq!(mid.value, 20);
    }

    #[test]
    fn number_item_get_returns_value() {
        let it = number_item(42, 0, 0, 100);
        assert_eq!(NumberItem_get(&it), 42);
    }

    #[test]
    fn number_item_decrease_clamps_at_min() {
        let mut it = number_item(1, 0, 0, 10);
        NumberItem_decrease(&mut it);
        assert_eq!(it.value, 0);
        // already at min: stays clamped
        NumberItem_decrease(&mut it);
        assert_eq!(it.value, 0);
    }

    #[test]
    fn number_item_increase_clamps_at_max() {
        let mut it = number_item(9, 0, 0, 10);
        NumberItem_increase(&mut it);
        assert_eq!(it.value, 10);
        NumberItem_increase(&mut it);
        assert_eq!(it.value, 10);
    }

    #[test]
    fn number_item_toggle_wraps_at_max() {
        let mut it = number_item(0, 0, 0, 2);
        NumberItem_toggle(&mut it);
        assert_eq!(it.value, 1);
        NumberItem_toggle(&mut it);
        assert_eq!(it.value, 2);
        // at max -> wrap back to min
        NumberItem_toggle(&mut it);
        assert_eq!(it.value, 0);
        // above max also wraps (C uses >=)
        it.value = 5;
        NumberItem_toggle(&mut it);
        assert_eq!(it.value, 0);
    }

    #[test]
    fn start_editing_saves_value_and_clears_buffer() {
        let mut it = number_item(7, 0, 0, 100);
        it.editBuffer = "stale".to_string();
        NumberItem_startEditing(&mut it);
        assert!(it.editing);
        assert_eq!(it.savedValue, 7);
        assert!(it.editBuffer.is_empty());
    }

    #[test]
    fn start_editing_from_value_integer() {
        let mut it = number_item(123, 0, 0, 1000);
        NumberItem_startEditingFromValue(&mut it);
        assert!(it.editing);
        assert_eq!(it.savedValue, 123);
        assert_eq!(it.editBuffer, "123");
    }

    #[test]
    fn start_editing_from_value_scaled_decimal() {
        // scale=-2 => display = 10^-2 * value = value/100, formatted %.2f
        let mut it = number_item(150, -2, 0, 10000);
        NumberItem_startEditingFromValue(&mut it);
        assert_eq!(it.editBuffer, "1.50");
    }

    #[test]
    fn start_editing_from_value_truncates_to_max() {
        // 12-digit value formats to 12 chars; buffer caps at 10.
        let mut it = number_item(0, 0, 0, i32::MAX);
        it.value = 2_000_000_000; // 10 digits, fits exactly
        NumberItem_startEditingFromValue(&mut it);
        assert_eq!(it.editBuffer, "2000000000");
        assert_eq!(it.editBuffer.len(), NUMBERITEM_EDIT_MAX);
    }

    #[test]
    fn cancel_editing_clears() {
        let mut it = number_item(3, 0, 0, 10);
        it.editing = true;
        it.editBuffer = "99".to_string();
        NumberItem_cancelEditing(&mut it);
        assert!(!it.editing);
        assert!(it.editBuffer.is_empty());
    }

    #[test]
    fn apply_editing_empty_buffer_returns_false() {
        let mut it = number_item(5, 0, 0, 10);
        it.editing = true;
        assert!(!NumberItem_applyEditing(&mut it));
        assert!(!it.editing);
        assert_eq!(it.value, 5); // unchanged
    }

    #[test]
    fn apply_editing_integer_commits_and_clamps() {
        let mut it = number_item(0, 0, 0, 50);
        it.editBuffer = "27".to_string();
        assert!(NumberItem_applyEditing(&mut it));
        assert_eq!(it.value, 27);
        assert!(it.editBuffer.is_empty());
        // above max clamps down
        it.editBuffer = "999".to_string();
        assert!(NumberItem_applyEditing(&mut it));
        assert_eq!(it.value, 50);
    }

    #[test]
    fn apply_editing_leading_numeric_prefix_like_strtol() {
        // strtol parses "12" and stops at 'a' (endptr != start => ok)
        let mut it = number_item(0, 0, 0, 100);
        it.editBuffer = "12abc".to_string();
        assert!(NumberItem_applyEditing(&mut it));
        assert_eq!(it.value, 12);
    }

    #[test]
    fn apply_editing_no_conversion_returns_false() {
        // endptr == start: no numeric prefix
        let mut it = number_item(9, 0, 0, 100);
        it.editBuffer = "abc".to_string();
        assert!(!NumberItem_applyEditing(&mut it));
        assert_eq!(it.value, 9); // unchanged
        assert!(it.editBuffer.is_empty());
    }

    #[test]
    fn apply_editing_scaled_decimal_round_trips() {
        // scale=-2: "1.50" -> round(1.5 / 0.01) = 150
        let mut it = number_item(0, -2, 0, 100000);
        it.editBuffer = "1.50".to_string();
        assert!(NumberItem_applyEditing(&mut it));
        assert_eq!(it.value, 150);
    }

    #[test]
    fn add_char_digits_and_bounds() {
        let mut it = number_item(0, 0, 0, i32::MAX);
        for d in b'0'..=b'9' {
            assert!(NumberItem_addChar(&mut it, d as char));
        }
        assert_eq!(it.editBuffer, "0123456789");
        assert_eq!(it.editBuffer.len(), NUMBERITEM_EDIT_MAX);
        // 11th char rejected (buffer full)
        assert!(!NumberItem_addChar(&mut it, '0'));
        assert_eq!(it.editBuffer.len(), NUMBERITEM_EDIT_MAX);
    }

    #[test]
    fn add_char_rejects_non_digit() {
        let mut it = number_item(0, 0, 0, 100);
        assert!(!NumberItem_addChar(&mut it, 'a'));
        assert!(!NumberItem_addChar(&mut it, '-'));
        assert!(it.editBuffer.is_empty());
    }

    #[test]
    fn add_char_dot_only_when_scale_negative() {
        // scale >= 0 rejects '.'
        let mut pos = number_item(0, 0, 0, 100);
        assert!(!NumberItem_addChar(&mut pos, '.'));
        // scale < 0 accepts a single '.', rejects the second
        let mut neg = number_item(0, -2, 0, 100);
        assert!(NumberItem_addChar(&mut neg, '1'));
        assert!(NumberItem_addChar(&mut neg, '.'));
        assert!(!NumberItem_addChar(&mut neg, '.'));
        assert!(NumberItem_addChar(&mut neg, '5'));
        assert_eq!(neg.editBuffer, "1.5");
    }

    #[test]
    fn add_char_comma_normalized_to_dot() {
        let mut it = number_item(0, -2, 0, 100);
        assert!(NumberItem_addChar(&mut it, ','));
        assert_eq!(it.editBuffer, ".");
        // second comma rejected (dot already present)
        assert!(!NumberItem_addChar(&mut it, ','));
    }

    #[test]
    fn delete_char_removes_last_and_is_safe_when_empty() {
        let mut it = number_item(0, 0, 0, 100);
        it.editBuffer = "12".to_string();
        NumberItem_deleteChar(&mut it);
        assert_eq!(it.editBuffer, "1");
        NumberItem_deleteChar(&mut it);
        assert!(it.editBuffer.is_empty());
        // no-op on empty buffer (C guards editLen > 0)
        NumberItem_deleteChar(&mut it);
        assert!(it.editBuffer.is_empty());
    }
}
