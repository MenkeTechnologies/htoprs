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
//! - `DynamicMeter_search` (`DynamicMeter.c:65`) — drives
//!   `Hashtable_foreach(dynamics, DynamicMeter_compare, &iter)` over the
//!   registry, writing the matched key through the out-param. Both
//!   `Hashtable_foreach` and the callback ([`DynamicMeter_compare`]) are
//!   ported now (`hashtable.rs`).
//! - `DynamicMeter_lookup` (`DynamicMeter.c:74`) — thin wrapper over
//!   `Hashtable_get`, returning the stored meter's `name`; ported now.
//! - `DynamicMeter_getCaption` (`DynamicMeter.c:92`) — `static` in C; reads
//!   `this->host->settings->dynamicMeters` via `Hashtable_get` and returns
//!   the entry's `caption`/`name` (else `this->caption`). Ported now —
//!   `Settings.dynamicMeters` and `Hashtable_get` are present.
//! - `DynamicMeter_getUiName` (`DynamicMeter.c:100`) — `static` in C; same
//!   `Hashtable_get` off `settings->dynamicMeters`, then `String_safeStrncpy`
//!   copies the caption (minus a trailing `": "`) or name into the
//!   caller-provided buffer. Ported now.
//!
//! Now ported (was stubbed) — thin wrappers over the per-platform
//! `Platform_dynamicMeter*` hooks, which are `static inline` no-ops in the
//! non-PCP build (`linux/Platform.h:128-138`):
//! - `DynamicMeters_new` (`DynamicMeter.c:39`) — returns
//!   `Platform_dynamicMeters()` (null `*mut Hashtable` in this build).
//! - `DynamicMeters_delete` (`DynamicMeter.c:43`) — calls
//!   `Platform_dynamicMetersDone` then `Hashtable_delete`.
//! - `DynamicMeter_init` (`DynamicMeter.c:79`) — `static` in C; thin
//!   wrapper over `Platform_dynamicMeterInit(meter)`.
//! - `DynamicMeter_updateValues` (`DynamicMeter.c:83`) — `static` in C;
//!   thin wrapper over `Platform_dynamicMeterUpdateValues(meter)`.
//! - `DynamicMeter_display` (`DynamicMeter.c:87`) — `static` in C; thin
//!   wrapper over `Platform_dynamicMeterDisplay(meter, out)`.
//!
//! `DynamicMeter_class` (the `MeterClass` vtable literal,
//! `DynamicMeter.c:119`) and its `DynamicMeter_attributes[]` colour table
//! (`DynamicMeter.c:27`) are not ported: `MeterClass` is the C
//! function-pointer dispatch table, which the port models via Rust's own
//! per-meter modules rather than a data literal of raw fn pointers.
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)] // preserve the C-style class name `DynamicMeter_class`
#![allow(dead_code)]

use crate::ported::hashtable::{Hashtable, Hashtable_delete, Hashtable_foreach, Hashtable_get};
use crate::ported::meter::Meter;
use crate::ported::object::{Object, ObjectClass, Object_class};
use crate::ported::richstring::RichString;
use crate::ported::xutils::String_safeStrncpy;

#[cfg(target_os = "macos")]
use crate::ported::darwin::platform::{
    Platform_dynamicMeters,
    Platform_dynamicMetersDone,
    Platform_dynamicMeterInit,
    Platform_dynamicMeterUpdateValues,
    Platform_dynamicMeterDisplay,
};
#[cfg(target_os = "linux")]
use crate::ported::linux::platform::{
    Platform_dynamicMeters,
    Platform_dynamicMetersDone,
    Platform_dynamicMeterInit,
    Platform_dynamicMeterUpdateValues,
    Platform_dynamicMeterDisplay,
};
#[cfg(target_os = "freebsd")]
use crate::ported::freebsd::platform::{
    Platform_dynamicMeters,
    Platform_dynamicMetersDone,
    Platform_dynamicMeterInit,
    Platform_dynamicMeterUpdateValues,
    Platform_dynamicMeterDisplay,
};
#[cfg(target_os = "netbsd")]
use crate::ported::netbsd::platform::{
    Platform_dynamicMeters,
    Platform_dynamicMetersDone,
    Platform_dynamicMeterInit,
    Platform_dynamicMeterUpdateValues,
    Platform_dynamicMeterDisplay,
};
#[cfg(target_os = "openbsd")]
use crate::ported::openbsd::platform::{
    Platform_dynamicMeters,
    Platform_dynamicMetersDone,
    Platform_dynamicMeterInit,
    Platform_dynamicMeterUpdateValues,
    Platform_dynamicMeterDisplay,
};
#[cfg(any(target_os = "solaris", target_os = "illumos"))]
use crate::ported::solaris::platform::{
    Platform_dynamicMeters,
    Platform_dynamicMetersDone,
    Platform_dynamicMeterInit,
    Platform_dynamicMeterUpdateValues,
    Platform_dynamicMeterDisplay,
};
#[cfg(target_os = "dragonfly")]
use crate::ported::dragonflybsd::platform::{
    Platform_dynamicMeters,
    Platform_dynamicMetersDone,
    Platform_dynamicMeterInit,
    Platform_dynamicMeterUpdateValues,
    Platform_dynamicMeterDisplay,
};
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
use crate::ported::unsupported::platform::{
    Platform_dynamicMeters,
    Platform_dynamicMetersDone,
    Platform_dynamicMeterInit,
    Platform_dynamicMeterUpdateValues,
    Platform_dynamicMeterDisplay,
};

/// Subset of htop's `DynamicMeter` struct (`DynamicMeter.h:17`).
///
/// `name` is the field `DynamicMeter_compare`, `DynamicMeter_search`, and
/// `DynamicMeter_lookup` touch; `caption` is read by
/// `DynamicMeter_getCaption`/`getUiName`. The C struct also carries
/// `description`, `type`, and `maximum`, modelled here for completeness.
#[derive(Debug, Clone, PartialEq)]
pub struct DynamicMeter {
    /// C: `char name[32]` — unique name, cannot contain spaces.
    pub name: String,
    /// C `char* caption` — short label shown before the meter value
    /// (`NULL` ⇒ `None`).
    pub caption: Option<String>,
    /// C `char* description` — longer explanatory text for the setup list
    /// (`NULL` ⇒ `None`).
    pub description: Option<String>,
    /// C `unsigned int type` — the meter display type id.
    pub type_: u32,
    /// C `double maximum` — the meter's value ceiling.
    pub maximum: f64,
}

/// Class descriptor for [`DynamicMeter`], present solely so a
/// `DynamicMeter` can be stored as a `Box<dyn Object>` value in the
/// ported [`Hashtable`] (whose value type is `dyn Object`). htop's
/// `Hashtable` stores raw `void*`, so C's `DynamicMeter` is **not** an
/// `Object` subclass — there is no `DynamicMeter_class` object vtable in
/// htop (the real `DynamicMeter_class` is a `MeterClass`, deliberately not
/// ported per the module note); this exists only as the safe-Rust adapter
/// for the ported table's owned `dyn Object` value model. Rooted at
/// [`Object_class`]; it sets no `display`/`compare` slots (the table never
/// dispatches through them).
static DynamicMeter_objectClass: ObjectClass = ObjectClass {
    extends: Some(&Object_class),
};

impl Object for DynamicMeter {
    fn klass(&self) -> &'static ObjectClass {
        &DynamicMeter_objectClass
    }
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

/// Port of `Hashtable* DynamicMeters_new(void)` from `DynamicMeter.c:39`.
/// Thin wrapper returning [`Platform_dynamicMeters`]; the non-PCP build's
/// `static inline` returns `NULL`, so this yields a null `*mut Hashtable`
/// (the C `Hashtable*` return maps to a raw pointer, matching
/// `Settings.dynamicMeters: Option<*mut Hashtable>`).
pub fn DynamicMeters_new() -> *mut Hashtable {
    Platform_dynamicMeters()
}

/// Port of `void DynamicMeters_delete(Hashtable* dynamics)` from
/// `DynamicMeter.c:43`. Null-guards the pointer (C `if (dynamics)`), runs the
/// platform teardown ([`Platform_dynamicMetersDone`], a non-PCP no-op), then
/// frees the table. C's `Hashtable_delete(dynamics)` takes ownership of the
/// heap `Hashtable*` and frees it; the ported [`Hashtable_delete`] consumes a
/// `Hashtable` by value, so the raw pointer is reclaimed with
/// `Box::from_raw` — the faithful analog of C's `free(dynamics)`. In the
/// non-PCP build `dynamics` is always null, so the guarded body never runs.
pub fn DynamicMeters_delete(dynamics: *mut Hashtable) {
    // C: if (dynamics) {
    if !dynamics.is_null() {
        // C: Platform_dynamicMetersDone(dynamics);
        Platform_dynamicMetersDone(dynamics);
        // C: Hashtable_delete(dynamics);
        // SAFETY: a non-null `dynamics` is a heap table owned by this call
        // (C's `Hashtable*`); reclaim it and hand it to Hashtable_delete by
        // value, which drops it (C's `free`).
        Hashtable_delete(unsafe { *Box::from_raw(dynamics) });
    }
}

/// Port of `bool DynamicMeter_search(Hashtable* dynamics, const char* name,
/// ht_key_t* key)` from `DynamicMeter.c:65`. Drives
/// [`Hashtable_foreach`] with the [`DynamicMeter_compare`] callback over the
/// registry, then writes the matched key through `key` when the out-param
/// is non-null (C `if (key) *key = iter.key`) and returns whether any entry
/// matched. A null table is skipped (C `if (dynamics)`), leaving `key`
/// unwritten from its `0` init and returning `false`.
///
/// The ported [`Hashtable_foreach`] hands each value to the callback as a
/// `&dyn Object`; C casts its `void*` straight to `const DynamicMeter*`, so
/// the safe-Rust analog downcasts through `Any` (the same round-trip the
/// stored `Box<dyn Object>` supports). Because `DynamicMeter_compare` only
/// records the `Copy` key and a `found` flag — never a borrow that must
/// escape — the single mutable `DynamicIterator` threads straight through
/// the closure, matching C's shared `&iter`.
pub fn DynamicMeter_search(
    dynamics: Option<&Hashtable>,
    name: &str,
    key: Option<&mut u32>,
) -> bool {
    // C: DynamicIterator iter = { .key = 0, .name = name, .found = false };
    let mut iter = DynamicIterator {
        name: name.to_string(),
        key: 0,
        found: false,
    };

    if let Some(dynamics) = dynamics {
        Hashtable_foreach(dynamics, &mut |k, value| {
            // C: const DynamicMeter* meter = (const DynamicMeter*)value;
            let any: &dyn core::any::Any = value;
            let meter = any
                .downcast_ref::<DynamicMeter>()
                .expect("DynamicMeter_search: hashtable value is not a DynamicMeter");
            DynamicMeter_compare(k, meter, &mut iter);
        });
    }

    // C: if (key) *key = iter.key;
    if let Some(key) = key {
        *key = iter.key;
    }

    // C: return iter.found;
    iter.found
}

/// Port of `const char* DynamicMeter_lookup(Hashtable* dynamics, ht_key_t key)`
/// from `DynamicMeter.c:74`. Thin wrapper over [`Hashtable_get`]: C casts
/// the returned `void*` to `const DynamicMeter*` and yields
/// `meter ? meter->name : NULL`; the safe-Rust analog downcasts the
/// `&dyn Object` value through `Any` and returns its `name` (or `None` on a
/// miss, C `NULL`).
pub fn DynamicMeter_lookup(dynamics: &Hashtable, key: u32) -> Option<&str> {
    Hashtable_get(dynamics, key)
        .and_then(|o| {
            let any: &dyn core::any::Any = o;
            any.downcast_ref::<DynamicMeter>()
        })
        .map(|meter| meter.name.as_str())
}

/// Port of `static void DynamicMeter_init(Meter* meter)` from
/// `DynamicMeter.c:79`. Thin wrapper over [`Platform_dynamicMeterInit`] (a
/// non-PCP no-op).
pub fn DynamicMeter_init(meter: &mut Meter) {
    Platform_dynamicMeterInit(meter);
}

/// Port of `static void DynamicMeter_updateValues(Meter* meter)` from
/// `DynamicMeter.c:83`. Thin wrapper over
/// [`Platform_dynamicMeterUpdateValues`] (a non-PCP no-op).
pub fn DynamicMeter_updateValues(meter: &mut Meter) {
    Platform_dynamicMeterUpdateValues(meter);
}

/// Port of `static void DynamicMeter_display(const Object* cast, RichString* out)`
/// from `DynamicMeter.c:87`. C casts the `Object*` to `const Meter*` and
/// forwards to [`Platform_dynamicMeterDisplay`] (a non-PCP no-op); the port
/// takes the already-typed `meter: &Meter` receiver directly — the sibling
/// panel/meter display-port convention — so the C upcast+downcast collapses
/// to the concrete reference.
pub fn DynamicMeter_display(meter: &Meter, out: &mut RichString) {
    // C: const Meter* meter = (const Meter*)cast;
    Platform_dynamicMeterDisplay(meter, out);
}

/// Port of `static const char* DynamicMeter_getCaption(const Meter* this)`
/// from `DynamicMeter.c:92`. Looks up
/// `this->host->settings->dynamicMeters` via [`Hashtable_get`] keyed on
/// `this->param` and returns `meter->caption ? meter->caption : meter->name`,
/// falling back to `this->caption` on a miss.
///
/// C reaches the registry through raw back-pointers (`this->host` is a
/// `*const Machine`, `settings->dynamicMeters` a `Hashtable*`); the port
/// mirrors that with `unsafe` derefs, and — like C — dereferences both
/// unconditionally (a NULL there is a C segfault, here an `expect` panic,
/// matching the `host->settings` precedent in `action.rs`). The stored
/// value comes back as `&dyn Object`; C casts its `void*` to
/// `const DynamicMeter*`, so the safe-Rust analog downcasts through `Any`.
/// The returned `&str` borrows `this` (its elided lifetime), the natural
/// analog of C's returned `const char*`.
pub fn DynamicMeter_getCaption(this: &Meter) -> &str {
    // C: const Settings* settings = this->host->settings;
    let settings = unsafe { (*this.host).settings.as_ref() }
        .expect("DynamicMeter_getCaption: host->settings is NULL");

    // C: const DynamicMeter* meter = Hashtable_get(settings->dynamicMeters, this->param);
    let dynamics: &Hashtable = unsafe {
        &*settings
            .dynamicMeters
            .expect("DynamicMeter_getCaption: settings->dynamicMeters is NULL")
    };
    let meter = Hashtable_get(dynamics, this.param).and_then(|o| {
        let any: &dyn core::any::Any = o;
        any.downcast_ref::<DynamicMeter>()
    });

    // C: if (meter) return meter->caption ? meter->caption : meter->name;
    if let Some(meter) = meter {
        return meter.caption.as_deref().unwrap_or(meter.name.as_str());
    }

    // C: return this->caption;
    &this.caption
}

/// Port of `static void DynamicMeter_getUiName(const Meter* this, char* name, size_t length)`
/// from `DynamicMeter.c:100`. Same `Hashtable_get` off
/// `settings->dynamicMeters` as [`DynamicMeter_getCaption`], then copies
/// the caption — minus a trailing `": "` (colon+space) — or, when the
/// entry has no caption, the name, into the caller-provided buffer via
/// [`String_safeStrncpy`]. On a registry miss (C `meter == NULL`) nothing
/// is written, leaving `name` as the caller passed it.
///
/// The C signature is `(char* name, size_t length)`: `name` is the
/// destination buffer and `length` its capacity. The port keeps that shape
/// (`name: &mut [u8]`, `length: usize`) rather than the `MeterGetUiName =
/// fn(&Meter) -> String` vtable shape, since this fn copies through
/// `String_safeStrncpy` whose `dest.len()` plays the C `size` arg — so the
/// sized copies pass `&mut name[..size]` sub-slices. C's `strlen` is a byte
/// count and its `MINIMUM` macro is `a < b ? a : b`, ported as
/// `bytes.len()` / `usize::min` (an inlined macro, not a new depth-0 fn).
pub fn DynamicMeter_getUiName(this: &Meter, name: &mut [u8], length: usize) {
    // C: assert(length > 0);
    assert!(length > 0);

    // C: const Settings* settings = this->host->settings;
    let settings = unsafe { (*this.host).settings.as_ref() }
        .expect("DynamicMeter_getUiName: host->settings is NULL");

    // C: const DynamicMeter* meter = Hashtable_get(settings->dynamicMeters, this->param);
    let dynamics: &Hashtable = unsafe {
        &*settings
            .dynamicMeters
            .expect("DynamicMeter_getUiName: settings->dynamicMeters is NULL")
    };
    let meter = Hashtable_get(dynamics, this.param).and_then(|o| {
        let any: &dyn core::any::Any = o;
        any.downcast_ref::<DynamicMeter>()
    });

    // C: if (meter) {
    if let Some(meter) = meter {
        // C: const char* uiName = meter->caption;
        if let Some(uiName) = meter.caption.as_deref() {
            let uiName = uiName.as_bytes();
            // C: size_t uiNameLen = strlen(uiName);
            let mut uiNameLen = uiName.len();
            // C: if (uiNameLen > 2 && uiName[uiNameLen - 2] == ':') uiNameLen -= 2;
            if uiNameLen > 2 && uiName[uiNameLen - 2] == b':' {
                uiNameLen -= 2;
            }
            // C: String_safeStrncpy(name, uiName, MINIMUM(length, uiNameLen + 1));
            let size = length.min(uiNameLen + 1);
            String_safeStrncpy(&mut name[..size], uiName);
        } else {
            // C: String_safeStrncpy(name, meter->name, length);
            String_safeStrncpy(&mut name[..length], meter.name.as_bytes());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meter(name: &str) -> DynamicMeter {
        DynamicMeter {
            name: name.to_string(),
            caption: None,
            description: None,
            type_: 0,
            maximum: 0.0,
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

    use crate::ported::hashtable::{Hashtable_new, Hashtable_put};

    // Build a registry the way DynamicMeters_new would: a non-owner table
    // keyed by meter index, values boxed as `dyn Object`.
    fn registry(names: &[(u32, &str)]) -> Hashtable {
        let mut ht = Hashtable_new(0, false);
        for &(k, n) in names {
            Hashtable_put(&mut ht, k, Box::new(meter(n)));
        }
        ht
    }

    #[test]
    fn lookup_returns_name_for_present_key() {
        let ht = registry(&[(1, "cpu"), (2, "mem"), (100, "io")]);
        assert_eq!(DynamicMeter_lookup(&ht, 1), Some("cpu"));
        assert_eq!(DynamicMeter_lookup(&ht, 2), Some("mem"));
        assert_eq!(DynamicMeter_lookup(&ht, 100), Some("io"));
    }

    #[test]
    fn lookup_returns_none_for_absent_key() {
        let ht = registry(&[(1, "cpu")]);
        assert_eq!(DynamicMeter_lookup(&ht, 999), None);
    }

    #[test]
    fn search_finds_by_name_and_writes_key() {
        let ht = registry(&[(10, "cpu"), (20, "mem"), (30, "io")]);
        let mut key: u32 = 0;
        assert!(DynamicMeter_search(Some(&ht), "mem", Some(&mut key)));
        assert_eq!(key, 20);
    }

    #[test]
    fn search_key_out_param_is_optional() {
        // C: `if (key)` — passing None (C NULL) must not fault.
        let ht = registry(&[(5, "cpu")]);
        assert!(DynamicMeter_search(Some(&ht), "cpu", None));
    }

    #[test]
    fn search_miss_returns_false_and_zeroes_key() {
        let ht = registry(&[(1, "cpu"), (2, "mem")]);
        let mut key: u32 = 12345;
        assert!(!DynamicMeter_search(
            Some(&ht),
            "nonexistent",
            Some(&mut key)
        ));
        // C leaves iter.key at its 0 init when nothing matches.
        assert_eq!(key, 0);
    }

    #[test]
    fn search_null_table_returns_false() {
        // C: `if (dynamics)` guards the foreach; a null table yields
        // iter.found == false and iter.key == 0.
        let mut key: u32 = 7;
        assert!(!DynamicMeter_search(None, "cpu", Some(&mut key)));
        assert_eq!(key, 0);
    }

    #[test]
    fn search_is_case_sensitive_like_string_eq() {
        // String_eq is strcmp==0: "CPU" != "cpu".
        let ht = registry(&[(1, "CPU")]);
        let mut key: u32 = 0;
        assert!(!DynamicMeter_search(Some(&ht), "cpu", Some(&mut key)));
        assert_eq!(key, 0);
        assert!(DynamicMeter_search(Some(&ht), "CPU", None));
    }

    #[test]
    fn search_result_matches_lookup_of_returned_key() {
        // The key written by search must round-trip through lookup to the
        // same meter name (the foreach/get paths must agree).
        let ht = registry(&[(3, "alpha"), (17, "beta"), (42, "gamma")]);
        for name in ["alpha", "beta", "gamma"] {
            let mut key: u32 = 0;
            assert!(DynamicMeter_search(Some(&ht), name, Some(&mut key)));
            assert_eq!(DynamicMeter_lookup(&ht, key), Some(name));
        }
    }
}
