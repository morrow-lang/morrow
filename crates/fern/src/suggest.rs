//! Bounded nearest-name suggestions for unresolved identifiers, fields and types.
//! Suggestions only decorate diagnostics; they never change resolution.

/// Names longer than this are not compared, keeping edit-distance work small and predictable.
const MAX_NAME_CHARS: usize = 64;
/// Candidate sets are truncated deterministically before comparison.
const MAX_CANDIDATES: usize = 4_096;

/// Return the closest candidate to `name`, or `None` when nothing is plausibly a typo.
///
/// A candidate qualifies when its edit distance is at most one third of the longer
/// spelling (at least one edit), or when it differs only by letter case. Ties select
/// the lexicographically smallest candidate so messages stay deterministic.
pub fn nearest<'a>(name: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let target: Vec<char> = name.chars().take(MAX_NAME_CHARS + 1).collect();
    if target.is_empty() || target.len() > MAX_NAME_CHARS {
        return None;
    }
    let mut best: Option<(usize, &str)> = None;
    for candidate in candidates.into_iter().take(MAX_CANDIDATES) {
        if candidate == name || candidate.is_empty() || candidate.starts_with('$') {
            continue;
        }
        let other: Vec<char> = candidate.chars().take(MAX_NAME_CHARS + 1).collect();
        if other.len() > MAX_NAME_CHARS {
            continue;
        }
        let distance = if candidate.eq_ignore_ascii_case(name) {
            0
        } else {
            let allowed = (target.len().max(other.len()) / 3).max(1);
            if target.len().abs_diff(other.len()) > allowed {
                continue;
            }
            let distance = levenshtein(&target, &other);
            if distance > allowed {
                continue;
            }
            distance
        };
        let better = match best {
            None => true,
            Some((best_distance, best_name)) => {
                distance < best_distance || (distance == best_distance && candidate < best_name)
            }
        };
        if better {
            best = Some((distance, candidate));
        }
    }
    best.map(|(_, candidate)| candidate.to_owned())
}

/// Format the suffix appended to a diagnostic, or an empty string without a suggestion.
pub fn hint<'a>(name: &str, candidates: impl IntoIterator<Item = &'a str>) -> String {
    nearest(name, candidates).map_or_else(String::new, |found| format!("; did you mean '{found}'?"))
}

/// Common spellings from other languages mapped to the Fern API verbs they usually mean.
const SYNONYMS: &[(&str, &[&str])] = &[
    ("add", &["push", "insert", "put"]),
    ("append", &["push", "concat"]),
    ("insert", &["put", "push"]),
    ("set", &["put"]),
    ("remove", &["delete"]),
    ("length", &["len"]),
    ("size", &["len"]),
    ("count", &["len"]),
    ("empty", &["is_empty"]),
    ("has", &["contains"]),
    ("includes", &["contains"]),
    ("has_key", &["contains"]),
    ("first", &["head"]),
    ("rest", &["tail"]),
    ("reduce", &["fold"]),
    ("foldl", &["fold"]),
    ("each", &["map"]),
    ("for_each", &["map"]),
    ("select", &["filter"]),
    ("where", &["filter"]),
    ("some", &["any"]),
    ("every", &["all"]),
    ("at", &["get"]),
    ("nth", &["at", "get"]),
    ("index", &["at", "get"]),
    ("skip", &["drop"]),
    ("limit", &["take"]),
    ("to_int", &["parse"]),
    ("upper", &["to_upper"]),
    ("uppercase", &["to_upper"]),
    ("upcase", &["to_upper"]),
    ("lower", &["to_lower"]),
    ("lowercase", &["to_lower"]),
    ("downcase", &["to_lower"]),
    ("strip", &["trim"]),
    ("substring", &["slice"]),
    ("substr", &["slice"]),
    ("find", &["index_of", "find"]),
    ("position", &["index_of"]),
    ("startswith", &["starts_with"]),
    ("endswith", &["ends_with"]),
    ("equals", &["eq"]),
    ("unwrap", &["unwrap_or"]),
    ("flat_map", &["map"]),
    ("read_text", &["read"]),
    ("write_text", &["write"]),
    ("open", &["read"]),
    ("fetch", &["get"]),
];

/// Suggest a builtin module member: nearest spelling first, then a well-known synonym.
/// `members` holds fully qualified spellings sharing `prefix`; the result is qualified too.
pub fn nearest_member<'a>(
    prefix: &str,
    member: &str,
    members: impl IntoIterator<Item = &'a str>,
) -> Option<String> {
    let members: Vec<&str> = members.into_iter().take(MAX_CANDIDATES).collect();
    let qualified = format!("{prefix}.{member}");
    if let Some(found) = nearest(&qualified, members.iter().copied()) {
        return Some(found);
    }
    let lowered = member.to_ascii_lowercase();
    SYNONYMS
        .iter()
        .find(|(alias, _)| *alias == lowered)
        .and_then(|(_, targets)| {
            targets.iter().find_map(|target| {
                let candidate = format!("{prefix}.{target}");
                members
                    .iter()
                    .any(|existing| *existing == candidate)
                    .then_some(candidate)
            })
        })
}

/// Return the unqualified tail of a module-qualified declaration name.
pub fn tail(name: &str) -> &str {
    name.rsplit('.').next().unwrap_or(name)
}

/// Optimal string alignment distance: insertions, deletions, substitutions and adjacent
/// transpositions each cost one edit, so `cuont` is one edit from `count`.
fn levenshtein(a: &[char], b: &[char]) -> usize {
    let width = b.len() + 1;
    let mut rows: Vec<Vec<usize>> = vec![(0..width).collect()];
    for (i, &left) in a.iter().enumerate() {
        let mut current = vec![0; width];
        current[0] = i + 1;
        for (j, &right) in b.iter().enumerate() {
            let previous = &rows[i];
            let substitution = previous[j] + usize::from(left != right);
            let mut best = substitution.min(previous[j + 1] + 1).min(current[j] + 1);
            if i > 0 && j > 0 && left == b[j - 1] && a[i - 1] == right {
                best = best.min(rows[i - 1][j - 1] + 1);
            }
            current[j + 1] = best;
        }
        rows.push(current);
    }
    rows[a.len()][b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_single_edit_typos_and_case_differences() {
        assert_eq!(
            nearest("cuont", ["count", "total"]),
            Some("count".to_owned())
        );
        assert_eq!(
            nearest("Lenght", ["len", "length"]),
            Some("length".to_owned())
        );
        assert_eq!(nearest("string", ["String"]), Some("String".to_owned()));
        assert_eq!(
            nearest("helpr", ["helper", "main"]),
            Some("helper".to_owned())
        );
        assert_eq!(nearest("Gren", ["Green", "Red"]), Some("Green".to_owned()));
    }

    #[test]
    fn transpositions_cost_one_edit() {
        assert_eq!(levenshtein(&['a', 'b'], &['b', 'a']), 1);
        assert_eq!(levenshtein(&['c', 'a'], &['a', 'b', 'c']), 3);
        assert_eq!(tail("main.helper"), "helper");
        assert_eq!(tail("helper"), "helper");
    }

    #[test]
    fn rejects_distant_or_identical_names() {
        assert_eq!(nearest("zzzzzzzzzz", ["count", "total"]), None);
        assert_eq!(nearest("count", ["count"]), None);
        assert_eq!(nearest("", ["count"]), None);
        assert_eq!(nearest("ab", ["xyz"]), None);
    }

    #[test]
    fn ties_resolve_lexicographically() {
        assert_eq!(nearest("cat", ["cut", "bat"]), Some("bat".to_owned()));
    }

    #[test]
    fn bounds_long_names_and_candidate_counts() {
        let long = "a".repeat(MAX_NAME_CHARS + 1);
        assert_eq!(nearest(&long, [long.as_str()]), None);
        let many = std::iter::repeat_n("zzzzzzzzzz", MAX_CANDIDATES).chain(["count"]);
        assert_eq!(nearest("cuont", many), None);
        let few = std::iter::repeat_n("zzzzzzzzzz", MAX_CANDIDATES - 1).chain(["count"]);
        assert_eq!(nearest("cuont", few), Some("count".to_owned()));
    }

    #[test]
    fn members_fall_back_to_synonyms_only_when_the_target_exists() {
        let members = ["Map.put", "Map.get", "Map.len"];
        assert_eq!(
            nearest_member("Map", "insert", members),
            Some("Map.put".to_owned())
        );
        assert_eq!(
            nearest_member("Map", "Size", members),
            Some("Map.len".to_owned())
        );
        assert_eq!(
            nearest_member("Map", "gett", members),
            Some("Map.get".to_owned())
        );
        assert_eq!(nearest_member("Map", "upper", members), None);
        assert_eq!(nearest_member("Map", "zzzzzzzz", members), None);
    }

    #[test]
    fn hint_formats_the_suffix() {
        assert_eq!(hint("helpr", ["helper"]), "; did you mean 'helper'?");
        assert_eq!(hint("zzz", ["helper"]), "");
    }
}
