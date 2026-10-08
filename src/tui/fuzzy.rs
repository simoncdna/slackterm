/// Matches `query` as a case-insensitive subsequence of `candidate`.
/// Returns a score (higher is better) and the char indices that matched.
pub fn fuzzy_match(query: &str, candidate: &str) -> Option<(i32, Vec<usize>)> {
    let query: Vec<char> = query.to_lowercase().chars().collect();
    let mut matched = Vec::with_capacity(query.len());
    let mut score = 0;
    let mut wanted = query.iter().peekable();
    let mut previous: Option<char> = None;

    for (index, c) in candidate.chars().enumerate() {
        let Some(&&q) = wanted.peek() else { break };
        if c.to_lowercase().eq(std::iter::once(q)) {
            score += 10;
            if matched.last() == Some(&(index.wrapping_sub(1))) {
                score += 15;
            }
            if previous.is_none_or(|p| !p.is_alphanumeric()) {
                score += 20;
            }
            matched.push(index);
            wanted.next();
        }
        previous = Some(c);
    }

    if wanted.peek().is_some() {
        return None;
    }
    let spread = match (matched.first(), matched.last()) {
        (Some(first), Some(last)) => (last - first) as i32,
        _ => 0,
    };
    Some((score - spread, matched))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_subsequences_case_insensitively() {
        let (_, indices) = fuzzy_match("dep", "# Deploys").unwrap();
        assert_eq!(indices, [2, 3, 4]);
        assert!(fuzzy_match("xyz", "# deploys").is_none());
    }

    #[test]
    fn prefers_contiguous_matches_at_word_starts() {
        let (contiguous, _) = fuzzy_match("dep", "# deploys").unwrap();
        let (scattered, _) = fuzzy_match("dep", "# data-exports").unwrap();
        assert!(contiguous > scattered);
    }

    #[test]
    fn an_empty_query_matches_everything() {
        assert_eq!(fuzzy_match("", "anything"), Some((0, vec![])));
    }
}
