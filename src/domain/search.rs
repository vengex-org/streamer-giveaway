//! Finding entrants by a rough name, so a streamer can check that someone got in.

/// How well `name` matches `query`, case-insensitively; lower is better, `None` is no match.
/// Whole name beats prefix, prefix beats a run inside the name, and that beats letters that
/// only appear in order ("mgn" finds "MegaNinja").
pub fn fuzzy_rank(query: &str, name: &str) -> Option<u8> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Some(0);
    }
    let name = name.to_lowercase();
    if name == query {
        Some(0)
    } else if name.starts_with(&query) {
        Some(1)
    } else if name.contains(&query) {
        Some(2)
    } else if is_subsequence(&query, &name) {
        Some(3)
    } else {
        None
    }
}

fn is_subsequence(query: &str, name: &str) -> bool {
    let mut letters = name.chars();
    query.chars().all(|q| letters.any(|n| n == q))
}

/// Indices of the names that match, best matches first and entry order within a rank.
pub fn search<'a>(query: &str, names: impl Iterator<Item = &'a str>) -> Vec<usize> {
    let mut hits: Vec<(u8, usize)> = names
        .enumerate()
        .filter_map(|(index, name)| fuzzy_rank(query, name).map(|rank| (rank, index)))
        .collect();
    hits.sort_unstable();
    hits.into_iter().map(|(_, index)| index).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closer_matches_rank_better() {
        assert_eq!(fuzzy_rank("meganinja", "MegaNinja"), Some(0));
        assert_eq!(fuzzy_rank("mega", "MegaNinja"), Some(1));
        assert_eq!(fuzzy_rank("ninja", "MegaNinja"), Some(2));
        assert_eq!(fuzzy_rank("mgn", "MegaNinja"), Some(3));
        assert_eq!(fuzzy_rank("ninjamega", "MegaNinja"), None);
    }

    #[test]
    fn search_orders_by_rank_then_entry_order() {
        let names = ["xNinja", "Ninja", "NinjaTurtle", "nxixnxjxa", "Other"];
        assert_eq!(search("ninja", names.into_iter()), [1, 2, 0, 3]);
    }

    #[test]
    fn a_blank_query_matches_everyone() {
        assert_eq!(search("  ", ["a", "b"].into_iter()), [0, 1]);
    }
}
