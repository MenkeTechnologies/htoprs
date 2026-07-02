//! Port of `UsersTable.c` — htop's uid → username cache.
//!
//! C names are preserved verbatim (htop uses `CamelCase_snake`), so
//! `non_snake_case` is allowed for the whole module — matching the spec
//! name-for-name is the point of the port. Each C function takes
//! `UsersTable* this`; the faithful analog is a free fn taking
//! `this: &mut UsersTable` / `this: &UsersTable`.
//!
//! # C model
//!
//! ```c
//! typedef struct UsersTable_ {
//!    Hashtable* users;   // uid -> char* username, owning
//! } UsersTable;
//! ```
//!
//! The single field is a `Hashtable` created with `Hashtable_new(10, true)`
//! (`owner == true`): the table owns the `char*` usernames it caches and
//! frees them on removal / teardown. Keys are uids (`ht_key_t` ==
//! `unsigned int`), values are `xStrdup`'d usernames.
//!
//! # Rust model
//!
//! `users` is a `HashMap<u32, String>`, the same choice `table.rs` makes
//! for its `Hashtable* table` field: the ported [`Hashtable`] stores
//! `Box<dyn Object>` values, but a username is a plain `char*`, not an
//! `Object`, so the faithful analog of an OWNING `Hashtable` of strings is
//! a `HashMap` that owns its `String` values — dropping a value *is* the C
//! `owner`-free. The `u32` key is C `ht_key_t` / `unsigned int`; the
//! `String` value is the `xStrdup`'d `pw_name`.
//!
//! [`Hashtable`]: crate::ported::hashtable::Hashtable
//!
//! # Ported
//! - `UsersTable_new` (`UsersTable.c:20`)
//! - `UsersTable_delete` (`UsersTable.c:27`) — `Hashtable_delete(users)` +
//!   `free(this)`, a pure teardown. Ported by taking `this` by value
//!   (move-in + `Drop` is the C `free`), following the `Hashtable_delete`
//!   convention: the moved-in `UsersTable` and its owning `HashMap` drop
//!   at end of scope, which is the inner `Hashtable_delete` (owner-free of
//!   each cached `String`) plus the outer `free(this)`.
//! - `UsersTable_getRef` (`UsersTable.c:32`)
//! - `UsersTable_foreach` (`UsersTable.c:49`, `inline`)
#![allow(non_snake_case)]
#![allow(dead_code)]

use std::collections::HashMap;

/// Port of `typedef struct UsersTable_` from `UsersTable.h:13`. The C
/// `Hashtable* users` (created owning, uid → `char*`) is modeled as an
/// owning `HashMap<u32, String>`: the map owns each cached username and
/// dropping a value is the C `owner`-free (see the module docs).
pub struct UsersTable {
    /// C `Hashtable* users`: uid → username cache, owning its strings.
    pub users: HashMap<u32, String>,
}

/// Port of `UsersTable* UsersTable_new(void)` from `UsersTable.c:20`.
/// Allocates the table with an empty user cache. The C
/// `Hashtable_new(10, true)` initial size hint of 10 is carried as the
/// `HashMap`'s reserved capacity; the `owner == true` flag is the map's
/// ownership of its `String` values (see the module docs).
pub fn UsersTable_new() -> UsersTable {
    UsersTable {
        users: HashMap::with_capacity(10),
    }
}

/// Port of `void UsersTable_delete(UsersTable* this)` from
/// `UsersTable.c:27`. Frees the `users` hashtable (C
/// `Hashtable_delete(this->users)`) then frees the struct (C
/// `free(this)`). Taking `this` by value is the faithful analog of
/// `free(this)`, following the [`Hashtable_delete`] port: the moved-in
/// `UsersTable` — and its owning `HashMap<u32, String>`, hence every
/// cached username `String` — drops at end of scope, which *is* the
/// inner `Hashtable_delete` (owner-free of each `char*`) plus the outer
/// `free(this)`.
///
/// [`Hashtable_delete`]: crate::ported::hashtable::Hashtable_delete
pub fn UsersTable_delete(_this: UsersTable) {}

/// Port of `char* UsersTable_getRef(UsersTable* this, unsigned int uid)`
/// from `UsersTable.c:32`. Looks up the cached username for `uid`; on a
/// cache miss it resolves the name via `getpwuid` (caching the `pw_name`,
/// or the empty string `""` when the uid is unknown) and stores it. The C
/// returns `NULL` for an empty/unknown name (`if (!name || !*name)`),
/// modeled as `None`, and otherwise a borrowed pointer into the table,
/// modeled as `Some(&str)` borrowing from `this`.
pub fn UsersTable_getRef(this: &mut UsersTable, uid: u32) -> Option<&str> {
    // C: `Hashtable_get`; on NULL, resolve via getpwuid and `Hashtable_put`
    // the `xStrdup`'d name (or `xStrdup("")` when getpwuid fails). The
    // `entry` API computes the value only on the miss branch, matching the
    // `if (name == NULL)` guard.
    let name = this.users.entry(uid).or_insert_with(|| {
        // C `const struct passwd* userData = getpwuid(uid);`
        let userData = unsafe { libc::getpwuid(uid as libc::uid_t) };
        if !userData.is_null() {
            // C `name = xStrdup(userData->pw_name);`
            let pw_name = unsafe { (*userData).pw_name };
            unsafe { std::ffi::CStr::from_ptr(pw_name) }
                .to_string_lossy()
                .into_owned()
        } else {
            // C `name = xStrdup("");`
            String::new()
        }
    });

    // C: `if (!name || !*name) return NULL; return name;` — the cached
    // entry stays put; only the returned pointer is NULL for an empty name.
    if name.is_empty() {
        None
    } else {
        Some(name.as_str())
    }
}

/// Port of `inline void UsersTable_foreach(UsersTable* this,
/// Hashtable_PairFunction f, void* userData)` from `UsersTable.c:49`.
/// Delegates to a walk of the `users` table, calling `f(uid, username)`
/// for every cached entry. The C `Hashtable_PairFunction (ht_key_t, void*,
/// void*)` callback plus its `userData` argument are modeled as a single
/// `&mut dyn FnMut(u32, &str)` closure — user data the C threads through
/// `userData` is captured by the closure instead, exactly as the
/// `Hashtable_foreach` port models the callback+context pair.
pub fn UsersTable_foreach(this: &UsersTable, f: &mut dyn FnMut(u32, &str)) {
    for (&uid, name) in this.users.iter() {
        f(uid, name.as_str());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_starts_empty() {
        let t = UsersTable_new();
        assert!(t.users.is_empty());
    }

    #[test]
    fn get_ref_caches_current_uid() {
        // uid 0 is root on every unix and always resolvable via getpwuid,
        // so this exercises the miss → resolve → cache → hit path.
        let mut t = UsersTable_new();
        let name = UsersTable_getRef(&mut t, 0).map(str::to_owned);
        assert_eq!(name.as_deref(), Some("root"));
        // the name is now cached under uid 0
        assert_eq!(t.users.get(&0).map(String::as_str), Some("root"));

        // a second lookup hits the cache and returns the same name
        assert_eq!(UsersTable_getRef(&mut t, 0), Some("root"));
    }

    #[test]
    fn get_ref_unknown_uid_caches_empty_and_returns_none() {
        // A very high uid is (essentially certainly) not a real account, so
        // getpwuid fails and the C caches `""` and returns NULL == None.
        let mut t = UsersTable_new();
        let uid = 4_000_000_000u32;
        assert_eq!(UsersTable_getRef(&mut t, uid), None);
        // the empty string is cached (so getpwuid is not retried)
        assert_eq!(t.users.get(&uid).map(String::as_str), Some(""));
        // still None on the cached-hit path (`!*name`)
        assert_eq!(UsersTable_getRef(&mut t, uid), None);
    }

    #[test]
    fn foreach_visits_every_entry() {
        let mut t = UsersTable_new();
        t.users.insert(1, "alice".to_string());
        t.users.insert(2, "bob".to_string());
        t.users.insert(3, String::new());

        let mut seen: Vec<(u32, String)> = Vec::new();
        UsersTable_foreach(&t, &mut |uid, name| seen.push((uid, name.to_string())));

        seen.sort_unstable();
        assert_eq!(
            seen,
            vec![
                (1, "alice".to_string()),
                (2, "bob".to_string()),
                (3, String::new()),
            ]
        );
    }
}
