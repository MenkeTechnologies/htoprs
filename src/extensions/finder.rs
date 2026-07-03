//! #2 — fuzzy process finder.
//!
//! Plugs into the overlay dispatch chokepoint at
//! `src/ported/screenmanager.rs:801` (`overlay::dispatch_key`) — an fzf-style
//! incremental overlay over the process list, versus htop's substring-only
//! filter. Pure matcher here; the overlay chrome reuses the existing
//! `extensions::overlay` ratatui buffer.

/// A scored match against one candidate.
#[derive(Clone, Debug, PartialEq)]
pub struct Match {
    /// Index into the input slice.
    pub idx: usize,
    pub score: i32,
    /// Char positions in the candidate that matched (for highlight).
    pub positions: Vec<usize>,
}

/// Score `query` against a single `candidate`.
///
/// Subsequence match (chars in order, gaps allowed). Bonuses: consecutive
/// runs, start-of-string, start-of-word (after `/ - _ . space`), exact case.
/// Returns `None` if `query` is not a subsequence of `candidate`.
pub fn score(query: &str, candidate: &str) -> Option<(i32, Vec<usize>)> {
    let q: Vec<char> = query.chars().collect();
    if q.is_empty() {
        return Some((0, Vec::new()));
    }
    let ql: Vec<char> = q.iter().flat_map(|c| c.to_lowercase()).collect();
    let cand: Vec<char> = candidate.chars().collect();

    let mut qi = 0usize;
    let mut total = 0i32;
    let mut positions = Vec::with_capacity(ql.len());
    let mut prev: Option<usize> = None;

    for (i, &ch) in cand.iter().enumerate() {
        if qi >= ql.len() {
            break;
        }
        let chl = ch.to_lowercase().next().unwrap_or(ch);
        if chl == ql[qi] {
            let mut bonus = 1;
            if prev == Some(i.wrapping_sub(1)) {
                bonus += 5; // consecutive
            }
            if i == 0 {
                bonus += 8; // head of string
            } else {
                let p = cand[i - 1];
                if matches!(p, '/' | '-' | '_' | '.' | ' ' | ':') {
                    bonus += 7; // start of word
                }
            }
            if ch == q[qi] {
                bonus += 1; // exact case
            }
            total += bonus;
            positions.push(i);
            prev = Some(i);
            qi += 1;
        }
    }

    if qi == ql.len() {
        // prefer shorter, denser candidates
        total -= ((cand.len() as i32) - (ql.len() as i32)).max(0) / 4;
        Some((total, positions))
    } else {
        None
    }
}

/// Rank `items` against `query`, best first. Non-matches are dropped.
/// Ties break by original index for stable ordering.
pub fn fuzzy(query: &str, items: &[String]) -> Vec<Match> {
    let mut out: Vec<Match> = items
        .iter()
        .enumerate()
        .filter_map(|(idx, s)| {
            score(query, s).map(|(sc, pos)| Match {
                idx,
                score: sc,
                positions: pos,
            })
        })
        .collect();
    out.sort_by(|a, b| b.score.cmp(&a.score).then(a.idx.cmp(&b.idx)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_query_matches_all() {
        let items = vec!["a".to_string(), "b".to_string()];
        assert_eq!(fuzzy("", &items).len(), 2);
    }

    #[test]
    fn non_subsequence_rejected() {
        assert!(score("xyz", "firefox").is_none());
    }

    #[test]
    fn consecutive_beats_scattered() {
        let (dense, _) = score("fox", "firefox").unwrap();
        // scattered with no separators, so no word-boundary bonuses inflate it
        let (sparse, _) = score("fox", "faoaxabc").unwrap();
        assert!(dense > sparse, "dense={dense} sparse={sparse}");
    }

    #[test]
    fn word_boundary_bonus() {
        // "pg" should rank the boundary hit in "postgres -D /var/pg" high
        let items = vec![
            "postgres -D /var/lib/pg".to_string(),
            "gnome-shell".to_string(),
        ];
        let ranked = fuzzy("pg", &items);
        assert_eq!(ranked[0].idx, 0);
    }

    #[test]
    fn case_insensitive_with_exact_bonus() {
        assert!(score("FF", "firefox").is_some());
        let (exact, _) = score("fi", "firefox").unwrap();
        let (mixed, _) = score("Fi", "firefox").unwrap();
        assert!(exact >= mixed);
    }
}
