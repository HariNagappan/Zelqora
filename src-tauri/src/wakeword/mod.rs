//! Wake-word matching. Pure functions so the behavior is unit-testable;
//! the audio pipeline feeds it normalized whisper transcripts.

use crate::util::{normalize_text, similarity};

#[derive(Debug, Clone, PartialEq)]
pub struct WakeMatch {
    pub matched: bool,
    /// Text spoken after the wake phrase in the same utterance (if any).
    pub command: Option<String>,
}

impl WakeMatch {
    fn no() -> Self {
        WakeMatch {
            matched: false,
            command: None,
        }
    }
}

/// Match a wake phrase inside a transcript with tolerant fuzzy matching.
///
/// * `fuzzy_ratio` — minimum average token similarity per phrase window
///   (from Settings; lower = more tolerant).
/// * Returns the residual command when the phrase is followed by more words
///   ("hey nova add milk" -> command "add milk").
pub fn match_wake(transcript: &str, phrase: &str, fuzzy_ratio: f32) -> WakeMatch {
    let phrase_norm = normalize_text(phrase);
    let text_norm = normalize_text(transcript);
    if phrase_norm.is_empty() || text_norm.is_empty() {
        return WakeMatch::no();
    }

    let phrase_tokens: Vec<&str> = phrase_norm.split(' ').collect();
    let text_tokens: Vec<&str> = text_norm.split(' ').collect();
    if text_tokens.len() < phrase_tokens.len() {
        return single_token_fallback(&text_tokens, &phrase_tokens);
    }

    let mut best: Option<(usize, f32)> = None;
    for start in 0..=(text_tokens.len() - phrase_tokens.len()) {
        let window = &text_tokens[start..start + phrase_tokens.len()];
        let mut total = 0.0f32;
        for (phrase_token, text_token) in phrase_tokens.iter().zip(window.iter()) {
            total += token_similarity(phrase_token, text_token);
        }
        let score = total / phrase_tokens.len() as f32;
        if best.map(|(_, previous)| score > previous).unwrap_or(true) {
            best = Some((start, score));
        }
    }

    let Some((start, score)) = best else {
        return WakeMatch::no();
    };
    if score < fuzzy_ratio {
        return WakeMatch::no();
    }

    let residual_tokens = &text_tokens[start + phrase_tokens.len()..];
    let command = if residual_tokens.is_empty() {
        None
    } else {
        Some(residual_tokens.join(" "))
    };
    WakeMatch {
        matched: true,
        command,
    }
}

/// Very short utterances like just "nova" should still wake when the last
/// phrase token matches almost exactly.
fn single_token_fallback(text_tokens: &[&str], phrase_tokens: &[&str]) -> WakeMatch {
    if phrase_tokens.len() < 2 || text_tokens.is_empty() {
        return WakeMatch::no();
    }
    let last = phrase_tokens[phrase_tokens.len() - 1];
    if text_tokens.len() == 1 {
        let score = token_similarity(last, text_tokens[0]);
        if score >= 0.9 {
            return WakeMatch {
                matched: true,
                command: None,
            };
        }
    }
    WakeMatch::no()
}

fn token_similarity(a: &str, b: &str) -> f32 {
    if a == b {
        return 1.0;
    }
    // Short tokens must be near-exact to count (avoids "no"/"nova" noise).
    if a.chars().count() <= 3 || b.chars().count() <= 3 {
        return if a.chars().count() == b.chars().count() && similarity(a, b) >= 0.66 {
            0.8
        } else {
            0.0
        };
    }
    similarity(a, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATIO: f32 = 0.75;

    #[test]
    fn matches_exact_wake_word() {
        let m = match_wake("Hey Nova", "hey nova", RATIO);
        assert!(m.matched);
        assert_eq!(m.command, None);
    }

    #[test]
    fn matches_with_punctuation_and_case() {
        let m = match_wake("Hey, Nova!", "hey nova", RATIO);
        assert!(m.matched);
    }

    #[test]
    fn extracts_residual_command() {
        let m = match_wake("Hey Nova add buy milk to my list", "hey nova", RATIO);
        assert!(m.matched);
        assert_eq!(m.command.as_deref(), Some("add buy milk to my list"));
    }

    #[test]
    fn tolerant_to_asr_variants() {
        // whisper sometimes hears "hey nova" as "hey noba".
        let m = match_wake("hey noba", "hey nova", RATIO);
        assert!(m.matched, "expected fuzzy match for 'hey noba'");

        let m = match_wake("a nova", "hey nova", RATIO);
        assert!(!m.matched, "'a nova' should NOT match at default strictness");
    }

    #[test]
    fn strictness_controls_tolerance() {
        let loose = match_wake("hey noba", "hey nova", 0.6);
        assert!(loose.matched);
        let strict = match_wake("hey noba", "hey nova", 0.95);
        assert!(!strict.matched);
    }

    #[test]
    fn ignores_unrelated_speech() {
        assert!(!match_wake("let's go get lunch", "hey nova", RATIO).matched);
        assert!(!match_wake("the novel was great", "hey nova", RATIO).matched);
    }

    #[test]
    fn single_word_fallback() {
        let m = match_wake("nova", "hey nova", RATIO);
        assert!(m.matched, "bare 'nova' should wake");
        let m = match_wake("novel", "hey nova", RATIO);
        assert!(!m.matched, "'novel' is too far from 'nova'");
    }

    #[test]
    fn custom_wake_word() {
        let m = match_wake("computer, what's on my list", "computer", RATIO);
        assert!(m.matched);
        assert_eq!(m.command.as_deref(), Some("whats on my list"));
    }
}
