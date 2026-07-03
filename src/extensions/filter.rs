//! #4 — regex + saved named filters.
//!
//! htop's filter is literal substring, not persistent. This adds regex
//! matching over a chosen field and a named store that serializes via the
//! `extensions::prefs` json pattern, so a filter survives across sessions.

use serde::{Deserialize, Serialize};

use crate::extensions::model::Proc;

/// Which field a filter tests.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Field {
    Comm,
    Cmdline,
    User,
    /// Match if any of comm/cmdline/user matches.
    Any,
}

/// A saved filter definition (serializable; carries no compiled state).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Filter {
    pub name: String,
    pub pattern: String,
    /// `true` = regex, `false` = case-insensitive substring.
    pub regex: bool,
    pub field: Field,
}

impl Filter {
    /// Compile into a matcher, validating the regex up front.
    pub fn compile(&self) -> Result<Compiled, regex::Error> {
        let re = if self.regex {
            Some(regex::Regex::new(&self.pattern)?)
        } else {
            None
        };
        Ok(Compiled {
            field: self.field,
            needle: self.pattern.to_lowercase(),
            re,
        })
    }
}

/// A ready-to-run filter. Regex is compiled once, not per row.
pub struct Compiled {
    field: Field,
    needle: String,
    re: Option<regex::Regex>,
}

impl Compiled {
    fn hit(&self, hay: &str) -> bool {
        match &self.re {
            Some(re) => re.is_match(hay),
            None => hay.to_lowercase().contains(&self.needle),
        }
    }

    /// Does `p` pass this filter?
    pub fn matches(&self, p: &Proc) -> bool {
        match self.field {
            Field::Comm => self.hit(&p.comm),
            Field::Cmdline => self.hit(&p.cmdline),
            Field::User => self.hit(&p.user),
            Field::Any => self.hit(&p.comm) || self.hit(&p.cmdline) || self.hit(&p.user),
        }
    }
}

/// Persistent collection of named filters.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FilterStore {
    pub filters: Vec<Filter>,
}

impl FilterStore {
    /// Insert or replace by name.
    pub fn put(&mut self, f: Filter) {
        match self.filters.iter_mut().find(|x| x.name == f.name) {
            Some(slot) => *slot = f,
            None => self.filters.push(f),
        }
    }

    pub fn get(&self, name: &str) -> Option<&Filter> {
        self.filters.iter().find(|f| f.name == name)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("filters serialize")
    }

    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }
}

/// Retain only rows passing `f`.
pub fn apply<'a>(f: &Compiled, table: &'a [Proc]) -> Vec<&'a Proc> {
    table.iter().filter(|p| f.matches(p)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extensions::model::synthetic_table;

    #[test]
    fn regex_over_cmdline() {
        let f = Filter {
            name: "rustc".into(),
            pattern: r"rustc\b.*edition".into(),
            regex: true,
            field: Field::Cmdline,
        }
        .compile()
        .unwrap();
        let table = synthetic_table(0);
        let hits = apply(&f, &table);
        assert!(hits.iter().any(|p| p.comm == "rustc"));
        assert!(!hits.iter().any(|p| p.comm == "zsh"));
    }

    #[test]
    fn substring_is_case_insensitive() {
        let f = Filter {
            name: "ff".into(),
            pattern: "FIRE".into(),
            regex: false,
            field: Field::Comm,
        }
        .compile()
        .unwrap();
        let table = synthetic_table(0);
        assert_eq!(apply(&f, &table).len(), 2); // firefox, firefox-tab
    }

    #[test]
    fn invalid_regex_surfaces_error() {
        let bad = Filter {
            name: "bad".into(),
            pattern: "(".into(),
            regex: true,
            field: Field::Any,
        };
        assert!(bad.compile().is_err());
    }

    #[test]
    fn store_put_replaces_and_roundtrips() {
        let mut s = FilterStore::default();
        s.put(Filter {
            name: "a".into(),
            pattern: "x".into(),
            regex: false,
            field: Field::Any,
        });
        s.put(Filter {
            name: "a".into(),
            pattern: "y".into(),
            regex: false,
            field: Field::Any,
        });
        assert_eq!(s.filters.len(), 1);
        assert_eq!(s.get("a").unwrap().pattern, "y");
        let back = FilterStore::from_json(&s.to_json()).unwrap();
        assert_eq!(back.get("a").unwrap().pattern, "y");
    }
}
