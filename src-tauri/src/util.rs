//! Small text utilities shared by wake-word matching, intent parsing and
//! fuzzy todo resolution.

/// Lowercase + strip punctuation + collapse whitespace.
/// Apostrophes are removed rather than spaced so "don't" -> "dont".
pub fn normalize_text(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut pending_space = false;
    for ch in input.chars() {
        if ch.is_alphanumeric() {
            if pending_space && !out.is_empty() {
                out.push(' ');
            }
            pending_space = false;
            for lower in ch.to_lowercase() {
                out.push(lower);
            }
        } else if ch == '\'' || ch == '\u{2019}' {
            // Drop apostrophes entirely: "don't" -> "dont".
            continue;
        } else {
            pending_space = true;
        }
    }
    out
}

/// Classic Levenshtein distance over chars.
pub fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut curr = vec![0usize; b.len() + 1];
    for i in 1..=a.len() {
        curr[0] = i;
        for j in 1..=b.len() {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b.len()]
}

/// Normalized similarity in [0, 1] based on Levenshtein distance.
pub fn similarity(a: &str, b: &str) -> f32 {
    let a = a.trim();
    let b = b.trim();
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let max = a.chars().count().max(b.chars().count());
    if max == 0 {
        return 1.0;
    }
    1.0 - (levenshtein(a, b) as f32 / max as f32)
}

/// Token-overlap + substring similarity for matching search queries to titles.
pub fn title_similarity(query: &str, title: &str) -> f32 {
    let q = normalize_text(query);
    let t = normalize_text(title);
    if q.is_empty() || t.is_empty() {
        return 0.0;
    }
    if t == q || t.contains(&q) {
        return 1.0;
    }
    let q_tokens_all: Vec<&str> = q.split(' ').filter(|s| !s.is_empty()).collect();
    let t_tokens: Vec<&str> = t.split(' ').filter(|s| !s.is_empty()).collect();
    if q_tokens_all.is_empty() || t_tokens.is_empty() {
        return 0.0;
    }
    // Drop filler words so requests like "the milk thing" fuzzy-match "buy milk".
    const FILLER: &[&str] = &[
        "the", "a", "an", "my", "please", "thing", "things", "stuff", "something",
        "task", "tasks", "todo", "todos", "item", "items", "reminder", "entry",
    ];
    let q_tokens: Vec<&str> = q_tokens_all
        .iter()
        .copied()
        .filter(|token| !FILLER.contains(token))
        .collect();
    let q_tokens = if q_tokens.is_empty() { q_tokens_all } else { q_tokens };

    let mut matched = 0usize;
    for qt in &q_tokens {
        let best = t_tokens
            .iter()
            .map(|tt| {
                if tt == qt {
                    1.0
                } else if qt.len() >= 4 && tt.len() >= 4 {
                    similarity(qt, tt)
                } else {
                    0.0
                }
            })
            .fold(0.0f32, f32::max);
        if best >= 0.75 {
            matched += 1;
        }
    }
    let overlap = matched as f32 / q_tokens.len() as f32;
    let direct = similarity(&q, &t) * 0.6;
    overlap.max(direct)
}

/// Shorten a string for display in a response (keeps whole words when possible).
pub fn truncate_for_speech(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out = String::new();
    let mut last_space = 0;
    for (idx, ch) in text.chars().enumerate() {
        if ch == ' ' {
            last_space = idx;
        }
        if idx >= max_chars {
            break;
        }
        out.push(ch);
    }
    if last_space > max_chars / 2 {
        out.truncate(out.char_indices().nth(last_space).map(|(i, _)| i).unwrap_or(out.len()));
    }
    format!("{}…", out.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_punctuation() {
        assert_eq!(normalize_text("Hey, Nova!"), "hey nova");
        assert_eq!(normalize_text("  HEY   no-va  "), "hey no va");
        assert_eq!(normalize_text("don't you dare"), "dont you dare");
        assert_eq!(normalize_text(""), "");
    }

    #[test]
    fn similarity_works() {
        assert!(similarity("hot", "hot") > 0.99);
        assert!(similarity("hot", "hut") > 0.6);
        assert!(similarity("hot", "completely") < 0.4);
    }

    #[test]
    fn title_similarity_handles_partial_queries() {
        assert!(title_similarity("dbms assignment", "finish DBMS assignment") > 0.9);
        assert!(title_similarity("milk", "buy milk") > 0.9);
        assert!(title_similarity("bank stuff", "buy milk") < 0.4);
    }
}
