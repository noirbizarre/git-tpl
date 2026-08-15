//! Turning a typo into a pointer.
//!
//! Extracted from `graph`, which had the only copy, because three places now
//! want it: an unknown reference in a manifest expression, an undeclared name
//! in a file body, and an answer key that names no question. All three are the
//! same mistake — a name that is almost right — and all three deserve the same
//! answer.

/// The closest known name, if one is close enough to be a typo.
///
/// A plain edit-distance heuristic rather than a dependency: the whole job is
/// to turn a typo into a pointer, and a near-miss suggestion is no worse than
/// none. The threshold is what keeps it from offering a *different* word,
/// which is worse than silence — a caller reads a suggestion as a fact.
pub fn closest<'a>(unknown: &str, known: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let unknown_lower = unknown.to_lowercase();
    known
        .into_iter()
        .map(|name| (edit_distance(&unknown_lower, &name.to_lowercase()), name))
        // A suggestion further away than a third of the name's length is not a
        // typo, it is a different word.
        .filter(|(distance, name)| *distance <= (name.len() / 3).max(1))
        .min_by_key(|(distance, _)| *distance)
        .map(|(_, name)| name.to_string())
}

/// Levenshtein distance.
pub fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    let mut current = vec![0; b.len() + 1];

    for (i, ca) in a.iter().enumerate() {
        current[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            current[j + 1] = (previous[j] + cost)
                .min(previous[j + 1] + 1)
                .min(current[j] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case("projct_name", &["project_name", "license"], Some("project_name"))]
    #[case("licence", &["project_name", "license"], Some("license"))]
    #[case("project_name", &["project_name"], Some("project_name"))]
    fn a_near_miss_is_suggested(
        #[case] unknown: &str,
        #[case] known: &[&str],
        #[case] expected: Option<&str>,
    ) {
        assert_eq!(closest(unknown, known.iter().copied()).as_deref(), expected);
    }

    /// A suggestion is read as a fact, so offering a different word is worse
    /// than offering nothing.
    #[rstest]
    #[case("totally_different", &["project_name", "license"])]
    #[case("x", &["project_name"])]
    fn a_distant_name_is_not_suggested(#[case] unknown: &str, #[case] known: &[&str]) {
        assert_eq!(closest(unknown, known.iter().copied()), None);
    }

    #[test]
    fn nothing_known_suggests_nothing() {
        assert_eq!(closest("anything", std::iter::empty()), None);
    }

    #[rstest]
    #[case("", "", 0)]
    #[case("abc", "abc", 0)]
    #[case("abc", "abx", 1)]
    #[case("kitten", "sitting", 3)]
    fn edit_distance_is_levenshtein(#[case] a: &str, #[case] b: &str, #[case] expected: usize) {
        assert_eq!(edit_distance(a, b), expected);
    }
}
