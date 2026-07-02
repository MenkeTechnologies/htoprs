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
//! Left as `todo!()` stubs (require unported substrate):
//! - `OptionItem_delete`, `TextItem_display`, `CheckItem_display`,
//!   `NumberItem_display` — depend on the `Object` vtable and
//!   `RichString`/`CRT_colors`, none of which are ported.
//! - `TextItem_new`, `CheckItem_newByRef`, `CheckItem_newByVal`,
//!   `NumberItem_newByRef`, `NumberItem_newByVal` — allocate through
//!   `AllocThis`/`xStrdup` and populate the `Object` super and `text`
//!   fields, which need the object substrate. Tests construct the
//!   structs directly via their public fields instead.
//!
//! Pointer-indirection limitation: the C `CheckItem`/`NumberItem` can
//! store either a direct value or a pointer to an external value
//! (`bool* ref` / `int* ref`, set by the `*_newByRef` constructors).
//! An aliasing raw pointer to an external cell cannot be modeled in
//! safe Rust without unsafe, so the structs here model only the
//! direct-value case (`ref == NULL` in C). Every accessor below ports
//! the `else` (direct) branch of its C body faithfully; the `if
//! (this->ref)` branch is intentionally NOT modeled (not faked).
#![allow(non_snake_case)]
#![allow(dead_code)]

/// Maximum number of characters in a [`NumberItem`] edit buffer.
/// Port of `#define NUMBERITEM_EDIT_MAX 10` from `OptionItem.h:14`.
pub const NUMBERITEM_EDIT_MAX: usize = 10;

/// Direct-value model of C `CheckItem` (`OptionItem.h:43`). The C struct
/// also carries `bool* ref` for pointer-indirection; that case is not
/// modeled in safe Rust (see module docs), so only `value` is kept.
pub struct CheckItem {
    pub value: bool,
}

/// Direct-value model of C `NumberItem` (`OptionItem.h:50`). The C
/// struct also carries `int* ref`; that pointer-indirection case is not
/// modeled (see module docs). The C `char editBuffer[NUMBERITEM_EDIT_MAX
/// + 1]` + `int editLen` pair is modeled as a single `String` whose
/// length is the C `editLen` (the two are always kept in lockstep in the
/// C source). The C `text` field is not modeled (needs object substrate).
pub struct NumberItem {
    pub value: i32,
    pub scale: i32,
    pub min: i32,
    pub max: i32,
    pub editing: bool,
    pub editBuffer: String,
    pub savedValue: i32,
}

/// TODO: port of `static void OptionItem_delete(Object* cast` from `OptionItem.c:23`.
pub fn OptionItem_delete() {
    todo!("port of OptionItem.c:23")
}

/// TODO: port of `static void TextItem_display(const Object* cast, RichString* out` from `OptionItem.c:31`.
pub fn TextItem_display() {
    todo!("port of OptionItem.c:31")
}

/// TODO: port of `static void CheckItem_display(const Object* cast, RichString* out` from `OptionItem.c:38`.
pub fn CheckItem_display() {
    todo!("port of OptionItem.c:38")
}

/// TODO: port of `static void NumberItem_display(const Object* cast, RichString* out` from `OptionItem.c:52`.
pub fn NumberItem_display() {
    todo!("port of OptionItem.c:52")
}

/// TODO: port of `TextItem* TextItem_new(const char* text` from `OptionItem.c:115`.
pub fn TextItem_new() {
    todo!("port of OptionItem.c:115")
}

/// TODO: port of `CheckItem* CheckItem_newByRef(const char* text, bool* ref` from `OptionItem.c:121`.
pub fn CheckItem_newByRef() {
    todo!("port of OptionItem.c:121")
}

/// TODO: port of `CheckItem* CheckItem_newByVal(const char* text, bool value` from `OptionItem.c:129`.
pub fn CheckItem_newByVal() {
    todo!("port of OptionItem.c:129")
}

/// Port of `CheckItem_get` from `OptionItem.c:137`. Ports the direct
/// (`ref == NULL`) branch; the pointer-indirection branch is not
/// modeled (see module docs).
pub fn CheckItem_get(this: &CheckItem) -> bool {
    this.value
}

/// Port of `CheckItem_set` from `OptionItem.c:145`. Ports the direct
/// branch; pointer indirection not modeled (see module docs).
pub fn CheckItem_set(this: &mut CheckItem, value: bool) {
    this.value = value;
}

/// Port of `CheckItem_toggle` from `OptionItem.c:153`. Ports the direct
/// branch; pointer indirection not modeled (see module docs).
pub fn CheckItem_toggle(this: &mut CheckItem) {
    this.value = !this.value;
}

/// TODO: port of `NumberItem* NumberItem_newByRef(const char* text, int* ref, int scale, int min, int max` from `OptionItem.c:161`.
pub fn NumberItem_newByRef() {
    todo!("port of OptionItem.c:161")
}

/// TODO: port of `NumberItem* NumberItem_newByVal(const char* text, int value, int scale, int min, int max` from `OptionItem.c:178`.
pub fn NumberItem_newByVal() {
    todo!("port of OptionItem.c:178")
}

/// Port of `NumberItem_get` from `OptionItem.c:195`. Ports the direct
/// (`ref == NULL`) branch; pointer indirection not modeled (see module
/// docs).
pub fn NumberItem_get(this: &NumberItem) -> i32 {
    this.value
}

/// Port of `NumberItem_decrease` from `OptionItem.c:203`. Decrements and
/// re-clamps to `[min, max]` (C `CLAMP`). Ports the direct branch;
/// pointer indirection not modeled (see module docs).
pub fn NumberItem_decrease(this: &mut NumberItem) {
    let v = this.value - 1;
    // CLAMP(x, low, high) == (x > high) ? high : (x < low ? low : x)
    this.value = if v > this.max {
        this.max
    } else if v < this.min {
        this.min
    } else {
        v
    };
}

/// Port of `NumberItem_increase` from `OptionItem.c:211`. Increments and
/// re-clamps to `[min, max]` (C `CLAMP`). Ports the direct branch;
/// pointer indirection not modeled (see module docs).
pub fn NumberItem_increase(this: &mut NumberItem) {
    let v = this.value + 1;
    this.value = if v > this.max {
        this.max
    } else if v < this.min {
        this.min
    } else {
        v
    };
}

/// Port of `NumberItem_toggle` from `OptionItem.c:219`. Steps by one,
/// wrapping back to `min` once at or above `max`. Ports the direct
/// branch; pointer indirection not modeled (see module docs).
pub fn NumberItem_toggle(this: &mut NumberItem) {
    if this.value >= this.max {
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
/// Ports the direct commit branch; pointer indirection not modeled.
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

    // CLAMP(newValue, min, max)
    this.value = if new_value > this.max {
        this.max
    } else if new_value < this.min {
        this.min
    } else {
        new_value
    };
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
            scale,
            min,
            max,
            editing: false,
            editBuffer: String::new(),
            savedValue: 0,
        }
    }

    #[test]
    fn check_item_get_set_toggle() {
        let mut it = CheckItem { value: false };
        assert!(!CheckItem_get(&it));
        CheckItem_set(&mut it, true);
        assert!(CheckItem_get(&it));
        CheckItem_toggle(&mut it);
        assert!(!CheckItem_get(&it));
        CheckItem_toggle(&mut it);
        assert!(CheckItem_get(&it));
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
