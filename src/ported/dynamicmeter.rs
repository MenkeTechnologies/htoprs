//! Partial port of `DynamicMeter.c` — htop's platform-provided dynamic
//! meter registry (name → `DynamicMeter`, rendered via the `Platform_*`
//! hooks).
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module — matching the spec
//! name-for-name is the point of the port. Each C function taking a
//! `Meter*`/`Hashtable*` ports to a free fn (the shape `Vector.c`/
//! `History.c` use), not a method.
//!
//! Ported (self-contained, no unported substrate):
//! - `DynamicMeter_compare` (`DynamicMeter.c:56`) — `static` in C; the
//!   `Hashtable_foreach` comparison callback. Its logic is a plain
//!   `String_eq` name match (`strcmp == 0`) plus an iterator update,
//!   needing no unported substrate.
//!
//! Stubbed (cannot be ported faithfully yet — specific blocker named):
//! - `DynamicMeters_new` (`DynamicMeter.c:39`) — returns
//!   `Platform_dynamicMeters()`, an unported `Platform_*` fn.
//! - `DynamicMeters_delete` (`DynamicMeter.c:43`) — calls
//!   `Platform_dynamicMetersDone` then `Hashtable_delete`; neither the
//!   `Platform_*` layer nor the `Hashtable` heap wrapper is ported
//!   (`hashtable.rs` ports only `nextPrime`).
//! - `DynamicMeter_search` (`DynamicMeter.c:65`) — drives
//!   `Hashtable_foreach(dynamics, DynamicMeter_compare, &iter)`; the
//!   `Hashtable` dispatch is not ported.
//! - `DynamicMeter_lookup` (`DynamicMeter.c:74`) — thin wrapper over
//!   `Hashtable_get`; same `Hashtable` blocker.
//! - `DynamicMeter_init` (`DynamicMeter.c:79`) — `static` in C; thin
//!   wrapper over `Platform_dynamicMeterInit(meter)`.
//! - `DynamicMeter_updateValues` (`DynamicMeter.c:83`) — `static` in C;
//!   thin wrapper over `Platform_dynamicMeterUpdateValues(meter)`.
//! - `DynamicMeter_display` (`DynamicMeter.c:87`) — `static` in C; thin
//!   wrapper over `Platform_dynamicMeterDisplay(meter, out)`; also needs
//!   the `Object`/`RichString` graph.
//! - `DynamicMeter_getCaption` (`DynamicMeter.c:92`) — `static` in C;
//!   reads `this->host->settings->dynamicMeters` via `Hashtable_get`.
//!   The `Settings` model has no `dynamicMeters` field and `Hashtable_get`
//!   is unported.
//! - `DynamicMeter_getUiName` (`DynamicMeter.c:100`) — `static` in C; same
//!   `Hashtable_get` off `settings->dynamicMeters`, plus
//!   `String_safeStrncpy` into a caller-provided `char*` buffer.
//!
//! `DynamicMeter_class` (the `MeterClass` vtable literal,
//! `DynamicMeter.c:119`) and its `DynamicMeter_attributes[]` colour table
//! (`DynamicMeter.c:27`) are not ported: `MeterClass` is the C
//! function-pointer dispatch table, which the port models via Rust's own
//! per-meter modules rather than a data literal of raw fn pointers.
#![allow(non_snake_case)]
#![allow(dead_code)]

/// Subset of htop's `DynamicMeter` struct (`DynamicMeter.h:17`).
///
/// Only `name` is modelled: it is the single field `DynamicMeter_compare`
/// touches. The C struct also carries `caption`, `description`, `type`,
/// and `maximum`, which this callback never reads and so are omitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DynamicMeter {
    /// C: `char name[32]` — unique name, cannot contain spaces.
    pub name: String,
}

/// Port of the file-local `DynamicIterator` struct (`DynamicMeter.c:50`).
///
/// `key` is `ht_key_t`, i.e. `unsigned int` (`Hashtable.h:14`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DynamicIterator {
    pub name: String,
    pub key: u32,
    pub found: bool,
}

/// Port of `DynamicMeter.c:56`.
///
/// `Hashtable_foreach` callback: `value` is the stored `DynamicMeter*`,
/// `data` is the `DynamicIterator*`. `String_eq(a, b)` is
/// `strcmp(a, b) == 0` (`XUtils.h:60`), i.e. exact string equality here.
/// Faithful to the C: on a match it sets `found` and records `key`, with
/// no early-out — a later matching key overwrites an earlier one.
pub fn DynamicMeter_compare(key: u32, meter: &DynamicMeter, iter: &mut DynamicIterator) {
    if iter.name == meter.name {
        iter.found = true;
        iter.key = key;
    }
}

/// TODO: port of `Hashtable* DynamicMeters_new(void)` from
/// `DynamicMeter.c:39`. Returns `Platform_dynamicMeters()`, an unported
/// `Platform_*` fn — no faithful body without the platform layer.
pub fn DynamicMeters_new() {
    todo!("port of DynamicMeter.c:39 — needs Platform_dynamicMeters()")
}

/// TODO: port of `void DynamicMeters_delete(Hashtable* dynamics)` from
/// `DynamicMeter.c:43`. Calls `Platform_dynamicMetersDone(dynamics)` then
/// `Hashtable_delete(dynamics)`; neither `Platform_*` nor the `Hashtable`
/// heap wrapper is ported (`hashtable.rs` ports only `nextPrime`).
pub fn DynamicMeters_delete() {
    todo!("port of DynamicMeter.c:43 — needs Platform_dynamicMetersDone + Hashtable_delete")
}

/// TODO: port of `bool DynamicMeter_search(Hashtable* dynamics, const char* name, ht_key_t* key)`
/// from `DynamicMeter.c:65`. Drives `Hashtable_foreach(dynamics,
/// DynamicMeter_compare, &iter)` over the registry; the `Hashtable`
/// dispatch is not ported. The callback ([`DynamicMeter_compare`]) is.
pub fn DynamicMeter_search() {
    todo!("port of DynamicMeter.c:65 — needs Hashtable_foreach")
}

/// TODO: port of `const char* DynamicMeter_lookup(Hashtable* dynamics, ht_key_t key)`
/// from `DynamicMeter.c:74`. Thin wrapper over `Hashtable_get(dynamics,
/// key)` returning the meter's `name` (or `NULL`); `Hashtable_get` is
/// unported.
pub fn DynamicMeter_lookup() {
    todo!("port of DynamicMeter.c:74 — needs Hashtable_get")
}

/// TODO: port of `static void DynamicMeter_init(Meter* meter)` from
/// `DynamicMeter.c:79`. Thin wrapper over `Platform_dynamicMeterInit(meter)`,
/// an unported `Platform_*` fn.
pub fn DynamicMeter_init() {
    todo!("port of DynamicMeter.c:79 — needs Platform_dynamicMeterInit")
}

/// TODO: port of `static void DynamicMeter_updateValues(Meter* meter)` from
/// `DynamicMeter.c:83`. Thin wrapper over
/// `Platform_dynamicMeterUpdateValues(meter)`, an unported `Platform_*` fn.
pub fn DynamicMeter_updateValues() {
    todo!("port of DynamicMeter.c:83 — needs Platform_dynamicMeterUpdateValues")
}

/// TODO: port of `static void DynamicMeter_display(const Object* cast, RichString* out)`
/// from `DynamicMeter.c:87`. Casts `cast` to `Meter*` and calls
/// `Platform_dynamicMeterDisplay(meter, out)`; needs the unported
/// `Platform_*` layer plus the `Object`/`RichString` graph.
pub fn DynamicMeter_display() {
    todo!("port of DynamicMeter.c:87 — needs Platform_dynamicMeterDisplay + RichString")
}

/// TODO: port of `static const char* DynamicMeter_getCaption(const Meter* this)`
/// from `DynamicMeter.c:92`. Looks up
/// `this->host->settings->dynamicMeters` via `Hashtable_get(.., this->param)`
/// and returns `meter->caption ? meter->caption : meter->name`, falling
/// back to `this->caption`. The `Settings` model has no `dynamicMeters`
/// field and `Hashtable_get` is unported.
pub fn DynamicMeter_getCaption() {
    todo!("port of DynamicMeter.c:92 — needs Settings.dynamicMeters + Hashtable_get")
}

/// TODO: port of `static void DynamicMeter_getUiName(const Meter* this, char* name, size_t length)`
/// from `DynamicMeter.c:100`. Same `Hashtable_get` off
/// `settings->dynamicMeters` as [`DynamicMeter_getCaption`], then copies
/// the caption (minus a trailing `": "`) or the name into the
/// caller-provided buffer via `String_safeStrncpy`. Blocked on the same
/// unported `Settings.dynamicMeters`/`Hashtable_get` substrate.
pub fn DynamicMeter_getUiName() {
    todo!("port of DynamicMeter.c:100 — needs Settings.dynamicMeters + Hashtable_get")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meter(name: &str) -> DynamicMeter {
        DynamicMeter {
            name: name.to_string(),
        }
    }

    fn iter(name: &str) -> DynamicIterator {
        DynamicIterator {
            name: name.to_string(),
            key: 0,
            found: false,
        }
    }

    #[test]
    fn match_sets_found_and_key() {
        let mut it = iter("cpu");
        DynamicMeter_compare(7, &meter("cpu"), &mut it);
        assert!(it.found);
        assert_eq!(it.key, 7);
    }

    #[test]
    fn no_match_leaves_iterator_untouched() {
        let mut it = iter("cpu");
        DynamicMeter_compare(7, &meter("mem"), &mut it);
        assert!(!it.found);
        assert_eq!(it.key, 0);
    }

    #[test]
    fn string_eq_is_exact_case_sensitive() {
        // String_eq == strcmp==0: case-sensitive, no trimming.
        let mut it = iter("CPU");
        DynamicMeter_compare(3, &meter("cpu"), &mut it);
        assert!(!it.found);
        assert_eq!(it.key, 0);
    }

    #[test]
    fn later_match_overwrites_key_no_early_out() {
        // C has no early return: iterating multiple matching entries
        // leaves `key` at the last one visited.
        let mut it = iter("cpu");
        DynamicMeter_compare(1, &meter("cpu"), &mut it);
        assert_eq!(it.key, 1);
        DynamicMeter_compare(2, &meter("cpu"), &mut it);
        assert!(it.found);
        assert_eq!(it.key, 2);
    }

    #[test]
    fn empty_names_compare_equal() {
        let mut it = iter("");
        DynamicMeter_compare(5, &meter(""), &mut it);
        assert!(it.found);
        assert_eq!(it.key, 5);
    }
}
