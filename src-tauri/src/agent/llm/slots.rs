//! Slot extraction for the offline parser: due dates, priorities and
//! categories inside an utterance, plus the leftover task title.
//!
//! Everything here is deterministic and unit-tested. The cloud provider gets
//! the same information from the tool schema; this module is what makes the
//! offline path understand "add finish my DBMS assignment due today at 6 pm".

use chrono::{Datelike, Duration, Local, NaiveDate, NaiveTime, TimeZone, Utc, Weekday};

/// What was extracted from an utterance body.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParsedSlots {
    /// The remaining task text (markers already removed).
    pub title: String,
    /// Absolute UTC due date as RFC 3339.
    pub due: Option<String>,
    /// `low` | `medium` | `high`
    pub priority: Option<String>,
    pub category: Option<String>,
}

/// Categories the parser recognises in "for …" / "… category" phrases.
const KNOWN_CATEGORIES: &[(&str, &str)] = &[
    ("college", "College"),
    ("university", "College"),
    ("school", "College"),
    ("study", "College"),
    ("career", "Career"),
    ("work", "Career"),
    ("job", "Career"),
    ("interview", "Career"),
    ("personal", "Personal"),
    ("home", "Personal"),
    ("health", "Personal"),
    ("errands", "Errands"),
    ("shopping", "Errands"),
    ("groceries", "Errands"),
];

const WEEKDAYS: &[(&str, Weekday)] = &[
    ("monday", Weekday::Mon),
    ("mon", Weekday::Mon),
    ("tuesday", Weekday::Tue),
    ("tue", Weekday::Tue),
    ("tues", Weekday::Tue),
    ("wednesday", Weekday::Wed),
    ("wed", Weekday::Wed),
    ("thursday", Weekday::Thu),
    ("thu", Weekday::Thu),
    ("thurs", Weekday::Thu),
    ("friday", Weekday::Fri),
    ("fri", Weekday::Fri),
    ("saturday", Weekday::Sat),
    ("sat", Weekday::Sat),
    ("sunday", Weekday::Sun),
    ("sun", Weekday::Sun),
];

/// Default time of day when the user only gives a date.
const DEFAULT_HOUR: u32 = 9;

fn bare(word: &str) -> String {
    word
        .trim_matches(|c: char| !c.is_alphanumeric() && c != ':')
        .to_ascii_lowercase()
}

/// Parse "6", "6pm", "6:30pm", "18:00" into (hour, minute).
fn parse_time(raw: &str) -> Option<(u32, u32)> {
    let cleaned = bare(raw);
    if cleaned.is_empty() {
        return None;
    }
    let (clock, suffix) = if let Some(rest) = cleaned.strip_suffix("pm") {
        (rest, Some("pm"))
    } else if let Some(rest) = cleaned.strip_suffix("am") {
        (rest, Some("am"))
    } else {
        (cleaned.as_str(), None)
    };
    let (hour_part, minute_part) = match clock.split_once(':') {
        Some((h, m)) => (h, Some(m)),
        None => (clock, None),
    };
    let mut hour: u32 = hour_part.parse().ok()?;
    let minute: u32 = match minute_part {
        Some(m) => m.parse().ok()?,
        None => 0,
    };
    if minute > 59 {
        return None;
    }
    match suffix {
        Some("pm") if hour < 12 => hour += 12,
        Some("am") if hour == 12 => hour = 0,
        Some(_) | None => {}
    }
    if hour > 23 {
        return None;
    }
    Some((hour, minute))
}

/// Resolve a local date into a UTC RFC 3339 timestamp.
fn to_utc(date: NaiveDate, hour: u32, minute: u32) -> Option<String> {
    let naive = NaiveTime::from_hms_opt(hour, minute, 0)?;
    let local = Local.from_local_datetime(&date.and_time(naive)).single()?;
    Some(local.with_timezone(&Utc).to_rfc3339_opts(
        chrono::SecondsFormat::Millis,
        true,
    ))
}

/// Weekday words (with optional "next"/"this") → the next matching date.
fn weekday_date(tokens: &[String], index: usize) -> Option<(NaiveDate, usize)> {
    let mut cursor = index;
    let mut modifier: Option<&str> = None;
    if let Some(word) = tokens.get(cursor) {
        if word == "next" || word == "this" || word == "coming" {
            modifier = Some(word.as_str());
            cursor += 1;
        }
    }
    let word = tokens.get(cursor)?;
    let weekday = WEEKDAYS
        .iter()
        .find(|(name, _)| name == word)
        .map(|(_, day)| *day)?;
    let today = Local::now().date_naive();
    let current = today.weekday().num_days_from_monday() as i64;
    let target = weekday.num_days_from_monday() as i64;
    let mut delta = (target - current + 7) % 7;
    if delta == 0 {
        // "friday" said on a Friday means next week, unless "this friday".
        delta = if modifier == Some("this") { 0 } else { 7 };
    }
    if modifier == Some("next") && delta < 7 {
        // "next friday" always means the week after this one.
        delta += 7;
    }
    Some((today + Duration::days(delta), cursor + 1))
}

/// Try to read a date (and optional time) starting at `index`.
/// Returns the resolved timestamp and the index just past the phrase.
fn read_date(tokens: &[String], index: usize) -> Option<(String, usize)> {
    let today = Local::now().date_naive();
    let first = tokens.get(index)?;
    let (date, mut cursor) = match first.as_str() {
        "today" | "tonight" => (today, index + 1),
        "tomorrow" | "tmrw" | "tmw" => (today + Duration::days(1), index + 1),
        "yesterday" => (today - Duration::days(1), index + 1),
        "weekend" => {
            let current = today.weekday().num_days_from_monday() as i64;
            let delta = (5 - current + 7) % 7;
            (today + Duration::days(delta.max(1)), index + 1)
        }
        "next" => {
            // "next week" / "next monday"
            match tokens.get(index + 1).map(|s| s.as_str()) {
                Some("week") => (today + Duration::days(7), index + 2),
                Some("month") => (today + Duration::days(30), index + 2),
                _ => weekday_date(tokens, index)?,
            }
        }
        _ => weekday_date(tokens, index)?,
    };

    // Optional time: "at 6pm", "6pm", "18:00"
    let mut hour = DEFAULT_HOUR;
    let mut minute = 0;
    let mut has_time = false;
    if first == "tonight" {
        hour = 18;
    }
    if tokens.get(cursor).map(|s| s.as_str()) == Some("at") {
        if let Some((h, m)) = tokens.get(cursor + 1).and_then(|t| parse_time(t)) {
            hour = h;
            minute = m;
            has_time = true;
            cursor += 2;
            // A trailing am/pm belongs to the time ("at 9:30 am").
            match tokens.get(cursor).map(|t| bare(t)).as_deref() {
                Some("am") => {
                    if hour == 12 {
                        hour = 0;
                    }
                    cursor += 1;
                }
                Some("pm") => {
                    if hour < 12 {
                        hour += 12;
                    }
                    cursor += 1;
                }
                _ => {}
            }
        }
    }
    if !has_time {
        if let Some((h, m)) = tokens.get(cursor).and_then(|t| parse_time(t)) {
            // A bare number only counts as a time when it looks like one
            // ("5 pm"); "2" in "chapter 2" must not become 02:00.
            let raw = bare(tokens.get(cursor).map(|s| s.as_str()).unwrap_or(""));
            if raw.ends_with("am") || raw.ends_with("pm") || raw.contains(':') {
                hour = h;
                minute = m;
                cursor += 1;
            }
        }
    }

    Some((to_utc(date, hour, minute)?, cursor))
}

/// Markers that introduce a due date.
const DUE_MARKERS: &[&str] = &["due", "by", "before", "on", "for", "until", "till"];

/// Extract the title plus any due date / priority / category from an utterance
/// body (the part after the "add" verb, with list markers already removed).
pub fn extract(body: &str) -> ParsedSlots {
    let originals: Vec<String> = body.split_whitespace().map(|s| s.to_string()).collect();
    let tokens: Vec<String> = originals.iter().map(|t| bare(t)).collect();
    let mut used = vec![false; tokens.len()];
    let mut due = None;
    let mut priority = None;
    let mut category = None;

    // ── priority: "high priority", "priority high", "urgent" ────────────────
    for i in 0..tokens.len() {
        let token = tokens[i].as_str();
        let value = match token {
            "high" | "urgent" | "important" => "high",
            "medium" | "normal" => "medium",
            "low" => "low",
            _ => continue,
        };
        let next_is_priority = tokens.get(i + 1).map(|t| t == "priority").unwrap_or(false);
        let prev_is_priority = i > 0 && tokens[i - 1] == "priority";
        if next_is_priority || prev_is_priority {
            priority = Some(value.to_string());
            used[i] = true;
            if next_is_priority {
                used[i + 1] = true;
            }
            if prev_is_priority {
                used[i - 1] = true;
            }
            break;
        }
    }

    // ── category: "for college", "college category", "category: college" ────
    for i in 0..tokens.len() {
        if used[i] {
            continue;
        }
        let token = tokens[i].as_str();
        if let Some((_, label)) = KNOWN_CATEGORIES.iter().find(|(name, _)| *name == token) {
            let next_is_category = tokens.get(i + 1).map(|t| t == "category").unwrap_or(false);
            let prev_is_category = i > 0 && tokens[i - 1] == "category";
            let introduced_by_for = i > 0 && tokens[i - 1] == "for";
            if next_is_category || prev_is_category || introduced_by_for {
                category = Some((*label).to_string());
                used[i] = true;
                if next_is_category {
                    used[i + 1] = true;
                }
                if prev_is_category {
                    used[i - 1] = true;
                }
                if introduced_by_for {
                    used[i - 1] = true;
                }
                break;
            }
        }
    }

    // ── due date ───────────────────────────────────────────────────────────
    // Preferred: an explicit marker ("due today at 6 pm", "by friday").
    'outer: for i in 0..tokens.len() {
        if used[i] || !DUE_MARKERS.contains(&tokens[i].as_str()) {
            continue;
        }
        // "for college" was already consumed as a category.
        for start in i + 1..tokens.len().min(i + 3) {
            if used[start] {
                continue;
            }
            if let Some((iso, end)) = read_date(&tokens, start) {
                due = Some(iso);
                used[i] = true;
                for slot in used.iter_mut().take(end).skip(start) {
                    *slot = true;
                }
                break 'outer;
            }
        }
    }
    // Fallback: a bare date phrase that ends the utterance
    // ("add call mom tomorrow", "call the bank tomorrow at 9:30 am").
    if due.is_none() {
        for start in 0..tokens.len() {
            if used.get(start).copied().unwrap_or(true) {
                continue;
            }
            if let Some((iso, end)) = read_date(&tokens, start) {
                if end >= tokens.len() {
                    due = Some(iso);
                    for slot in used.iter_mut().take(end).skip(start) {
                        *slot = true;
                    }
                }
                break;
            }
        }
    }

    let title = originals
        .iter()
        .enumerate()
        .filter(|(i, _)| !used[*i])
        .map(|(_, token)| token.as_str())
        .collect::<Vec<_>>()
        .join(" ");

    ParsedSlots {
        title: tidy(&title),
        due,
        priority,
        category,
    }
}

/// Collapse whitespace and strip dangling punctuation/connectors.
fn tidy(text: &str) -> String {
    let trimmed = text
        .trim_matches(|c: char| matches!(c, '.' | ',' | '!' | '?' | ';' | ':' | '"' | '\'' | ' '))
        .to_string();
    let mut out = String::with_capacity(trimmed.len());
    let mut last_space = false;
    for ch in trimmed.chars() {
        if ch.is_whitespace() {
            if !last_space {
                out.push(' ');
            }
            last_space = true;
        } else {
            out.push(ch);
            last_space = false;
        }
    }
    let cleaned = out.trim().to_string();
    // Drop trailing connectors left behind by a removed date phrase.
    for tail in [" at", " on", " by", " due", " for", " to"] {
        if let Some(stripped) = cleaned.strip_suffix(tail) {
            return stripped.trim().to_string();
        }
    }
    cleaned
}

/// Parse a standalone date phrase (used by EDIT_TODO: "move X to tomorrow").
pub fn parse_due_phrase(text: &str) -> Option<String> {
    let tokens: Vec<String> = text.split_whitespace().map(|t| bare(t)).collect();
    for start in 0..tokens.len() {
        if let Some((iso, _)) = read_date(&tokens, start) {
            return Some(iso);
        }
    }
    None
}

/// Summary scope words for SUMMARIZE_TODOS.
pub fn parse_scope(text: &str) -> Option<&'static str> {
    let lower = text.to_ascii_lowercase();
    if lower.contains("overdue") || lower.contains("past due") || lower.contains("late") {
        return Some("overdue");
    }
    if lower.contains("tomorrow") {
        return Some("tomorrow");
    }
    if lower.contains("this week") || lower.contains("next week") || lower.contains("the week") {
        return Some("week");
    }
    if lower.contains("today") || lower.contains("tonight") || lower.contains("right now") {
        return Some("today");
    }
    if lower.contains("everything") || lower.contains("all of my") || lower.contains("my whole") {
        return Some("all");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn due_of(text: &str) -> String {
        extract(text)
            .due
            .unwrap_or_else(|| panic!("expected a due date in: {text}"))
    }

    fn local_due(text: &str) -> String {
        let iso = due_of(text);
        chrono::DateTime::parse_from_rfc3339(&iso)
            .unwrap()
            .with_timezone(&Local)
            .format("%Y-%m-%d %H:%M")
            .to_string()
    }

    #[test]
    fn extracts_due_today_with_time() {
        let slots = extract("finish my DBMS assignment due today 6pm");
        assert_eq!(slots.title, "finish my DBMS assignment");
        let today = Local::now().date_naive();
        assert_eq!(local_due("finish my DBMS assignment due today 6pm"), format!("{today} 18:00"));
    }

    #[test]
    fn extracts_due_tomorrow_and_at_phrases() {
        let slots = extract("call the bank tomorrow at 9:30 am");
        assert_eq!(slots.title, "call the bank");
        let tomorrow = Local::now().date_naive() + Duration::days(1);
        assert!(local_due("call the bank tomorrow at 9:30 am").starts_with(&tomorrow.to_string()));

        let slots = extract("submit the report by friday at 5 pm");
        assert_eq!(slots.title, "submit the report");
        assert!(slots.due.is_some());
    }

    #[test]
    fn bare_weekday_dates_resolve_forward() {
        let slots = extract("revise operating systems on monday");
        assert_eq!(slots.title, "revise operating systems");
        let iso = slots.due.expect("monday should resolve");
        let when = chrono::DateTime::parse_from_rfc3339(&iso).unwrap();
        assert!(when > chrono::Utc::now(), "must be in the future");
    }

    #[test]
    fn extracts_priority_and_category() {
        let slots = extract("prepare for my technical interview high priority for career");
        assert_eq!(slots.title, "prepare for my technical interview");
        assert_eq!(slots.priority.as_deref(), Some("high"));
        assert_eq!(slots.category.as_deref(), Some("Career"));

        let slots = extract("finish the DBMS lab low priority");
        assert_eq!(slots.title, "finish the DBMS lab");
        assert_eq!(slots.priority.as_deref(), Some("low"));
    }

    #[test]
    fn plain_titles_are_untouched() {
        for text in [
            "buy milk",
            "finish DBMS assignment",
            "finish the DBMS lab",
            "call mom",
            "prepare for my technical interview",
        ] {
            let slots = extract(text);
            assert_eq!(slots.title, text, "title changed for {text}");
            assert!(slots.due.is_none(), "unexpected due for {text}");
            assert!(slots.priority.is_none());
            assert!(slots.category.is_none(), "unexpected category for {text}");
        }
    }

    #[test]
    fn numbers_are_not_mistaken_for_times() {
        let slots = extract("read chapter 2 of the DBMS book");
        assert_eq!(slots.title, "read chapter 2 of the DBMS book");
        assert!(slots.due.is_none());
    }

    #[test]
    fn parse_due_phrase_handles_edit_requests() {
        assert!(parse_due_phrase("tomorrow").is_some());
        assert!(parse_due_phrase("next monday at 8am").is_some());
        assert!(parse_due_phrase("the interview task").is_none());
    }

    #[test]
    fn scopes_match_the_designed_questions() {
        assert_eq!(parse_scope("what do I need to finish today"), Some("today"));
        assert_eq!(parse_scope("what's on my list for tomorrow"), Some("tomorrow"));
        assert_eq!(parse_scope("what is overdue"), Some("overdue"));
        assert_eq!(parse_scope("what's due this week"), Some("week"));
        assert_eq!(parse_scope("summarize my tasks"), None);
    }
}
