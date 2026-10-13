//! Offline, deterministic intent matcher used when no cloud AI is configured
//! (or as a fallback when the cloud is unreachable).
//!
//! This is deliberately NOT a list of hardcoded full phrases. It works on
//! generalized structural patterns: verb synonym sets, object-class words,
//! list-marker phrases and fuzzy pronoun resolution through the conversation
//! context. It covers a broad set of natural phrasings for the registered
//! commands and says "no match" otherwise (the agent then responds politely).

use serde_json::{json, Value};

use crate::commands::CommandRequest;

use super::super::context::ConversationContext;
use super::slots::{extract, parse_due_phrase, parse_scope};

/// Interpret an utterance offline. Returns `None` when nothing matches.
pub fn interpret(text: &str, context: &ConversationContext) -> Option<CommandRequest> {
    let cleaned = strip_leadins(text);
    if cleaned.trim().is_empty() {
        return None;
    }
    let lower = cleaned.to_ascii_lowercase();

    try_screenshot(&lower)
        .or_else(|| try_shutdown(&lower))
        .or_else(|| try_cancel_shutdown(&lower))
        .or_else(|| try_clear_completed(&lower))
        .or_else(|| try_todo_summarize(&lower))
        .or_else(|| try_todo_edit(&lower, &cleaned))
        .or_else(|| try_todo_search(&lower, &cleaned))
        .or_else(|| try_todo_list(&lower))
        .or_else(|| try_todo_complete(&lower, &cleaned, context))
        .or_else(|| try_todo_delete(&lower, &cleaned, context))
        .or_else(|| try_todo_add(&lower, &cleaned))
        .or_else(|| try_open(&lower, &cleaned))
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

pub(crate) fn strip_leadins(text: &str) -> String {
    let mut s = text.trim().to_string();
    const PREFIXES: &[&str] = &[
        "hey nova ", "hello nova ", "ok nova ", "okay nova ", "hi nova ", "nova ",
        "please ", "can you please ", "can you ", "could you please ", "could you ",
        "would you please ", "would you ", "will you ", "can u ",
    ];
    loop {
        let lower = s.to_ascii_lowercase();
        let mut changed = false;
        for prefix in PREFIXES {
            if lower.starts_with(prefix) {
                s = s[prefix.len()..].trim_start().to_string();
                changed = true;
                break;
            }
        }
        if !changed {
            break;
        }
    }
    const SUFFIXES: &[&str] = &[" please", " for me", " thanks", " thank you"];
    loop {
        let lower = s.to_ascii_lowercase();
        let mut changed = false;
        for suffix in SUFFIXES {
            if lower.ends_with(suffix) {
                let new_len = s.len() - suffix.len();
                s.truncate(new_len);
                s = s.trim_end().to_string();
                changed = true;
                break;
            }
        }
        if !changed {
            break;
        }
    }
    s.trim().to_string()
}

/// Case-insensitive prefix strip; returns the remainder from the original text.
fn strip_prefix_ci<'a>(text: &'a str, lower_prefix: &str) -> Option<&'a str> {
    let lower = text.to_ascii_lowercase();
    if lower.starts_with(lower_prefix) {
        Some(text[lower_prefix.len()..].trim_start())
    } else {
        None
    }
}

/// Phrases that mark the end of a todo title / query.
const LIST_MARKERS: &[&str] = &[
    " to my to-dos",
    " to my to-do list",
    " to my to do list",
    " to my todo list",
    " to my todos",
    " to my list",
    " to my tasks",
    " to my task list",
    " to the list",
    " to the todo list",
    " to my checklist",
    " on my to-dos",
    " on my to-do list",
    " on my to do list",
    " on my todo list",
    " on my todos",
    " on my list",
    " on my tasks",
    " on my task list",
    " in my todo list",
    " in my to-dos",
    " in my todos",
    " in my list",
    " in my tasks",
    " from my todo list",
    " from my list",
    " from my tasks",
    " from the list",
    " off my list",
    " off the list",
    " out of my list",
];

fn cut_at_markers(text: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let mut cut = text.len();
    for marker in LIST_MARKERS {
        if let Some(idx) = lower.find(marker) {
            if idx < cut {
                cut = idx;
            }
        }
    }
    text[..cut].trim().to_string()
}

fn strip_leading_articles(text: &str) -> String {
    let mut s = text.trim().to_string();
    loop {
        let lower = s.to_ascii_lowercase();
        let mut changed = false;
        for article in ["the ", "a ", "an ", "my "] {
            if lower.starts_with(article) && s.len() > article.len() {
                s = s[article.len()..].to_string();
                changed = true;
                break;
            }
        }
        if !changed {
            break;
        }
    }
    s
}

fn strip_trailing_nouns(text: &str) -> String {
    let mut s = text.trim().to_string();
    loop {
        let lower = s.to_ascii_lowercase();
        let mut changed = false;
        for noun in [" task", " todo", " to-do", " item", " reminder", " entry"] {
            if lower.ends_with(noun) {
                s.truncate(s.len() - noun.len());
                s = s.trim_end().to_string();
                changed = true;
                break;
            }
        }
        if !changed {
            break;
        }
    }
    s
}

fn tidy(text: &str) -> String {
    let trimmed = text
        .trim()
        .trim_matches(|c: char| matches!(c, '.' | ',' | '!' | '?' | ';' | ':' | '"' | '\'' | ' '));
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
    out.trim().to_string()
}

fn clean_add_title(raw: &str) -> Option<String> {
    let title = tidy(&cut_at_markers(raw));
    if title.chars().count() < 2 || title.chars().count() > 300 {
        return None;
    }
    if !title.chars().any(|c| c.is_alphabetic()) {
        return None;
    }
    const STOP: &[&str] = &[
        "it", "that", "this", "them", "those", "something", "anything", "a task", "task", "a todo",
        "todo", "an item", "item", "stuff", "things",
    ];
    let normalized = tidy(&title.to_ascii_lowercase());
    if STOP.contains(&normalized.as_str()) {
        return None;
    }
    Some(title)
}

fn clean_query(raw: &str) -> Option<String> {
    let stripped = strip_trailing_nouns(&cut_at_markers(raw));
    let without_articles = strip_leading_articles(&stripped);
    let cleaned = tidy(&without_articles);
    if cleaned.chars().count() < 2 {
        return None;
    }
    Some(cleaned)
}

/// Words that mean "the previously mentioned todo".
const PRONOUNS: &[&str] = &[
    "it", "that", "this", "them", "those", "the one", "that one", "the other one",
    "the same one", "the last one",
];

fn pronoun_target(raw: &str) -> bool {
    let normalized = tidy(&strip_leading_articles(&raw.to_ascii_lowercase()));
    PRONOUNS.contains(&normalized.as_str())
}

// ---------------------------------------------------------------------------
// Individual intents
// ---------------------------------------------------------------------------

fn try_screenshot(lower: &str) -> Option<CommandRequest> {
    let has_screenshot = lower.contains("screenshot") || lower.contains("screen shot");
    let capture_screen = (lower.contains("capture") || lower.contains("take"))
        && (lower.contains("screen") || lower.contains("screenshot"));
    if has_screenshot || capture_screen {
        return Some(CommandRequest {
            intent: "TAKE_SCREENSHOT".into(),
            parameters: json!({}),
        });
    }
    None
}

fn try_clear_completed(lower: &str) -> Option<CommandRequest> {
    let verb = ["clear", "remove", "delete", "clean up", "cleanup", "purge"]
        .iter()
        .any(|v| lower.contains(v));
    let completed = ["completed", "finished", "done", "checked"]
        .iter()
        .any(|v| lower.contains(v));
    let list_context = lower.contains("todo")
        || lower.contains("task")
        || lower.contains("list")
        || lower.contains("checklist");
    if verb && completed && list_context {
        return Some(CommandRequest {
            intent: "CLEAR_COMPLETED_TODOS".into(),
            parameters: json!({}),
        });
    }
    None
}

fn try_todo_search(lower: &str, orig: &str) -> Option<CommandRequest> {
    // The design is explicit that Nova does not search the web: such requests
    // must fall through to "that isn't supported yet" instead of being turned
    // into a task search that quietly finds nothing.
    const WEB_WORDS: &[&str] = &[
        "web",
        "internet",
        "google",
        "online",
        "browser",
        "wikipedia",
        "youtube",
    ];
    if WEB_WORDS.iter().any(|word| lower.contains(word)) {
        return None;
    }

    let has_todo_context = lower.contains("todo")
        || lower.contains("task")
        || lower.contains("my list")
        || lower.contains("checklist");

    let is_search = lower.starts_with("search ") || lower.contains("search for ");
    let is_find = lower.starts_with("find ") || lower.starts_with("look for ") || lower.starts_with("look up ");

    if !is_search && !is_find {
        return None;
    }
    // "find"/"look for" without todo context is too ambiguous.
    if is_find && !has_todo_context {
        return None;
    }

    let mut query: Option<String> = None;

    if let Some(idx) = lower.find(" for ") {
        let after = &orig[idx + 5..];
        query = Some(after.to_string());
    }
    if query.is_none() {
        for verb in ["search my todos for ", "search my tasks for ", "search my list for "] {
            if let Some(rest) = strip_prefix_ci(orig, verb) {
                query = Some(rest.to_string());
                break;
            }
        }
    }
    if query.is_none() {
        for verb in ["search ", "find ", "look for ", "look up "] {
            if let Some(rest) = strip_prefix_ci(orig, verb) {
                let rest = strip_prefix_ci(rest, "for ").unwrap_or(rest);
                query = Some(rest.to_string());
                break;
            }
        }
    }

    let raw_query = query?;
    // Trim list-context tails ("... in my todos").
    let cleaned = clean_query(&raw_query)?;
    if cleaned.is_empty() {
        return None;
    }
    Some(CommandRequest {
        intent: "SEARCH_TODOS".into(),
        parameters: json!({ "query": cleaned }),
    })
}

fn try_shutdown(lower: &str) -> Option<CommandRequest> {
    let verbs = [
        "shut down",
        "shutdown",
        "power off",
        "power down",
        "turn off",
        "switch off",
    ];
    let targets = ["computer", "pc", "laptop", "machine", "system", "windows"];
    // Never shut anything down that the user did not name as the machine.
    if lower.contains("nova") || lower.contains("shut down the app") {
        return None;
    }
    let has_verb = verbs.iter().any(|verb| lower.contains(verb));
    let has_target = targets.iter().any(|target| lower.contains(target));
    if !has_verb || !has_target {
        return None;
    }
    Some(CommandRequest {
        intent: "SHUTDOWN_PC".into(),
        parameters: json!({ "delaySeconds": 10 }),
    })
}

fn try_cancel_shutdown(lower: &str) -> Option<CommandRequest> {
    let cancel = ["cancel", "abort", "stop", "call off"]
        .iter()
        .any(|verb| lower.contains(verb));
    if !cancel || !lower.contains("shutdown") {
        return None;
    }
    Some(CommandRequest {
        intent: "CANCEL_SHUTDOWN".into(),
        parameters: json!({}),
    })
}

fn try_todo_summarize(lower: &str) -> Option<CommandRequest> {
    let has_context = [
        "list", "task", "todo", "to-do", "schedule", "due", "overdue", "today", "tomorrow",
    ]
    .iter()
    .any(|keyword| lower.contains(keyword));
    if !has_context {
        return None;
    }

    const REQUIRE_TODAY: &[&str] = &[
        "what do i need to finish",
        "what do i need to do",
        "what do i have to do",
        "what do i have ",
        "what have i got",
    ];
    const SUMMARY_WORDS: &[&str] = &[
        "summarize",
        "summary",
        "how many tasks",
        "whats due",
        "what's due",
        "what is due",
        "anything due",
        "whats left",
        "what's left",
        "how is my day",
        "what does my day",
    ];

    let scope = if REQUIRE_TODAY.iter().any(|pre| lower.contains(pre)) {
        parse_scope(lower).unwrap_or("today")
    } else if lower.contains("overdue") && (lower.starts_with("what") || lower.contains("anything"))
    {
        "overdue"
    } else if SUMMARY_WORDS.iter().any(|word| lower.contains(word)) {
        parse_scope(lower).unwrap_or("today")
    } else if lower.starts_with("what") {
        // "what's on my list for tomorrow" — a scope word makes it a summary,
        // a bare "what's on my list" stays a plain list request.
        parse_scope(lower)?
    } else {
        return None;
    };

    Some(CommandRequest {
        intent: "SUMMARIZE_TODOS".into(),
        parameters: json!({ "scope": scope }),
    })
}

/// Edit an existing task: rename, re-prioritise, reschedule or re-categorise.
fn try_todo_edit(lower: &str, orig: &str) -> Option<CommandRequest> {
    let has_context = lower.contains("task")
        || lower.contains("todo")
        || lower.contains("to-do")
        || lower.contains("my list")
        || lower.contains("reminder");

    // ── 1. explicit field setters: "set the due date of X to tomorrow" ────
    for verb in ["set the ", "change the "] {
        if let Some(rest) = strip_prefix_ci(orig, verb) {
            let rest_lower = rest.to_ascii_lowercase();
            for (field, kind) in [
                ("priority of ", "priority"),
                ("due date of ", "due"),
                ("deadline of ", "due"),
                ("category of ", "category"),
            ] {
                if let Some(after_field) = rest_lower
                    .strip_prefix(field)
                    .map(|_| &rest[field.len()..])
                {
                    if let Some((target, value)) = split_once_ci(after_field, " to ") {
                        if let Some(query) = clean_query(target) {
                            return edit_request(
                                &query,
                                kind,
                                edit_value(kind, value),
                                false,
                            );
                        }
                    }
                }
            }
        }
    }

    // ── 2. priority: "set X to high priority", "make X high priority" ─────
    for verb in ["set ", "mark ", "make ", "change ", "raise ", "lower "] {
        if let Some(rest) = strip_prefix_ci(orig, verb) {
            if let Some((value, start)) = find_priority_phrase(rest) {
                let target = trim_target(&rest[..start]);
                if let Some(query) = clean_query(&target) {
                    return edit_request(&query, "priority", value, false);
                }
            }
        }
    }

    // ── 3. reschedule: "move X to tomorrow", "set X due friday" ───────────
    for verb in ["move ", "reschedule ", "push ", "shift ", "set "] {
        if let Some(rest) = strip_prefix_ci(orig, verb) {
            let (target, when) = match split_once_ci(rest, " to ") {
                Some(parts) => parts,
                None => match split_once_ci(rest, " due ") {
                    Some(parts) => parts,
                    None => match split_once_ci(rest, " for ") {
                        Some(parts) => parts,
                        None => continue,
                    },
                },
            };
            if let Some(due) = parse_due_phrase(when) {
                if let Some(query) = clean_query(target) {
                    return edit_request(&query, "due", due, false);
                }
            }
        }
    }

    // ── 4. clear the due date ─────────────────────────────────────────────
    for verb in [
        "clear the due date on ",
        "clear the due date for ",
        "remove the due date from ",
        "remove the due date on ",
    ] {
        if let Some(rest) = strip_prefix_ci(orig, verb) {
            if let Some(query) = clean_query(rest) {
                return edit_request(&query, "due", String::new(), true);
            }
        }
    }

    // ── 5. rename: "rename X to Y", "change X to Y" ──────────────────────
    // "rename"/"retitle" can only mean a task, so they need no extra context;
    // the generic "change X to Y" requires list context to stay unambiguous.
    for verb in ["rename ", "retitle ", "change "] {
        if let Some(rest) = strip_prefix_ci(orig, verb) {
            if verb.starts_with("change") && !has_context {
                continue;
            }
            let (target, new_title) = match split_once_ci(rest, " to ") {
                Some(parts) => parts,
                None => continue,
            };
            let target_lower = target.to_ascii_lowercase();
            // "change the priority of X to high" is handled above.
            if ["priority", "due date", "deadline", "category"]
                .iter()
                .any(|field| target_lower.contains(field))
            {
                continue;
            }
            let query = clean_query(target)?;
            let title = tidy(new_title);
            if title.chars().count() < 2 {
                continue;
            }
            return edit_request(&query, "title", title, false);
        }
    }

    // ── 6. category: "categorize X as College", "set X category to College" ─
    for verb in ["categorize ", "category "] {
        if let Some(rest) = strip_prefix_ci(orig, verb) {
            let (target, value) = match split_once_ci(rest, " as ")
                .or_else(|| split_once_ci(rest, " to "))
            {
                Some(parts) => parts,
                None => continue,
            };
            let (query, category) = match clean_query(target) {
                Some(query) => (query, tidy(value)),
                None => continue,
            };
            if category.chars().count() < 2 {
                continue;
            }
            return edit_request(&query, "category", category, false);
        }
    }

    None
}

/// Build an EDIT_TODO request for one change.
fn edit_request(query: &str, kind: &str, value: String, clear_due: bool) -> Option<CommandRequest> {
    let mut params = serde_json::Map::new();
    params.insert("searchQuery".into(), json!(query));
    match kind {
        "title" => {
            params.insert("title".into(), json!(value));
        }
        "priority" => {
            params.insert("priority".into(), json!(value));
        }
        "due" => {
            if clear_due {
                params.insert("clearDue".into(), json!(true));
            } else {
                if value.is_empty() {
                    return None;
                }
                params.insert("due".into(), json!(value));
            }
        }
        "category" => {
            params.insert("category".into(), json!(value));
        }
        _ => return None,
    }
    Some(CommandRequest {
        intent: "EDIT_TODO".into(),
        parameters: Value::Object(params),
    })
}

fn edit_value(kind: &str, value: &str) -> String {
    match kind {
        "priority" => priority_value(value).unwrap_or_else(|| "medium".into()),
        "due" => parse_due_phrase(value).unwrap_or_default(),
        _ => tidy(value),
    }
}

/// Trim verb leftovers from a target phrase ("set the interview task to" → "the interview task").
/// Find "<priority> priority" in a fragment; returns the value and its offset.
fn find_priority_phrase(text: &str) -> Option<(String, usize)> {
    let lower = text.to_ascii_lowercase();
    let mut best: Option<(String, usize)> = None;
    for (word, value) in [
        ("high", "high"),
        ("urgent", "high"),
        ("important", "high"),
        ("medium", "medium"),
        ("normal", "medium"),
        ("low", "low"),
    ] {
        let marker = format!("{word} priority");
        if let Some(index) = lower.find(&marker) {
            let better = best.as_ref().map(|(_, at)| index < *at).unwrap_or(true);
            if better {
                best = Some((value.to_string(), index));
            }
        }
    }
    best
}

fn priority_value(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    if lower.contains("high") || lower.contains("urgent") {
        return Some("high".into());
    }
    if lower.contains("low") {
        return Some("low".into());
    }
    if lower.contains("medium") || lower.contains("normal") {
        return Some("medium".into());
    }
    None
}

/// Trim verb leftovers from a target phrase ("set the interview task to" → "the interview task").
fn trim_target(text: &str) -> String {
    let mut out = text.trim().to_string();
    for tail in [" to", " as", " into", " as the", " to the", " the"] {
        if out.to_ascii_lowercase().ends_with(tail) {
            out.truncate(out.len() - tail.len());
            out = out.trim().to_string();
        }
    }
    out
}

/// Case-insensitive split on the first occurrence of `needle`.
fn split_once_ci<'a>(text: &'a str, needle: &str) -> Option<(&'a str, &'a str)> {
    let lower = text.to_ascii_lowercase();
    let index = lower.find(needle)?;
    Some((&text[..index], &text[index + needle.len()..]))
}

fn try_todo_list(lower: &str) -> Option<CommandRequest> {
    let has_todo_context = lower.contains("todo")
        || lower.contains("to do list")
        || lower.contains("to-do")
        || lower.contains("task")
        || lower.contains("my list")
        || lower.contains("checklist");
    if !has_todo_context {
        return None;
    }

    const VERB_STARTS: &[&str] = &[
        "what", "list", "show", "see", "view", "display", "read", "tell me", "give me", "check",
        "read out", "run through",
    ];
    const VERB_CONTAINS: &[&str] = &[
        "whats on", "what's on", "what is on", "what are my", "show me", "list my", "show my",
        "tell me what", "give me my", "read my", "display my",
    ];

    let starts_ok = VERB_STARTS.iter().any(|v| lower.starts_with(v));
    let contains_ok = VERB_CONTAINS.iter().any(|v| lower.contains(v));
    if !starts_ok && !contains_ok {
        return None;
    }

    // A search request should not be mistaken for a list request.
    if lower.contains("search") || lower.contains("find") || lower.contains("look for") {
        return None;
    }

    Some(CommandRequest {
        intent: "LIST_TODOS".into(),
        parameters: json!({}),
    })
}

fn try_todo_complete(
    lower: &str,
    orig: &str,
    context: &ConversationContext,
) -> Option<CommandRequest> {
    // Pattern: "mark X as done/completed/finished" (also "mark X done").
    if let Some(rest) = strip_prefix_ci(orig, "mark ") {
        let rest_lower = rest.to_ascii_lowercase();
        if let Some(idx) = rest_lower.find(" as ") {
            let target = &rest[..idx];
            return complete_from_target(target, context);
        }
        // "mark X done / completed / finished"
        for suffix in [" as completed", " completed", " done", " finished", " complete"] {
            if rest_lower.ends_with(suffix) {
                let target = &rest[..rest.len() - suffix.len()];
                return complete_from_target(target, context);
            }
        }
        return None;
    }

    // Pattern: verb-first "complete/finish/check off/tick off X".
    for verb in [
        "i finished ",
        "i've finished ",
        "ive finished ",
        "i completed ",
        "i've completed ",
        "ive completed ",
        "i'm done with ",
        "im done with ",
        "done with ",
        "complete ",
        "completed ",
        "finish ",
        "finished ",
        "check off ",
        "tick off ",
        "cross off ",
    ] {
        if let Some(rest) = strip_prefix_ci(orig, verb) {
            return complete_from_target(rest, context);
        }
    }

    let _ = lower;
    None
}

fn complete_from_target(target: &str, context: &ConversationContext) -> Option<CommandRequest> {
    if pronoun_target(target) {
        return context.last_todo_id().map(|id| CommandRequest {
            intent: "COMPLETE_TODO".into(),
            parameters: json!({ "todoId": id }),
        });
    }
    let query = clean_query(target)?;
    Some(CommandRequest {
        intent: "COMPLETE_TODO".into(),
        parameters: json!({ "searchQuery": query }),
    })
}

fn try_todo_delete(
    lower: &str,
    orig: &str,
    context: &ConversationContext,
) -> Option<CommandRequest> {
    // Guard: file/folder deletion is not supported — never misroute it.
    let mentions_filesystem = lower.contains("file")
        || lower.contains("folder")
        || lower.contains("directory")
        || lower.contains("downloads")
        || lower.contains("documents")
        || lower.contains("desktop folder")
        || lower.contains("recycle bin");
    if mentions_filesystem {
        return None;
    }

    for verb in [
        "delete ",
        "remove ",
        "cancel ",
        "drop ",
        "get rid of ",
        "throw away ",
        "delete the ",
        "remove the ",
    ] {
        if let Some(rest) = strip_prefix_ci(orig, verb) {
            if pronoun_target(rest) {
                return context.last_todo_id().map(|id| CommandRequest {
                    intent: "DELETE_TODO".into(),
                    parameters: json!({ "todoId": id }),
                });
            }
            // An explicit "delete <name>" is unambiguous: file and folder
            // deletion was refused above, and tasks are the only thing Nova can
            // delete — so no additional context word is required ("delete the
            // DBMS assignment" must work, not just "delete the DBMS todo").
            let query = clean_query(rest)?;
            return Some(CommandRequest {
                intent: "DELETE_TODO".into(),
                parameters: json!({ "searchQuery": query }),
            });
        }
    }
    None
}

fn try_todo_add(lower: &str, orig: &str) -> Option<CommandRequest> {
    let mut body: Option<&str> = None;

    // Action verbs: "add X …", "put X …", "note down X …"
    for verb in [
        "add a task to ",
        "add a todo to ",
        "add to my list ",
        "add ",
        "put ",
        "append ",
        "note down ",
        "write down ",
        "jot down ",
        "create a todo ",
        "create todo ",
        "make a todo ",
        "make a task ",
    ] {
        if let Some(rest) = strip_prefix_ci(orig, verb) {
            body = Some(rest);
            break;
        }
    }

    // Reminder phrasings: "remind me to X", "remind me that I need to X".
    if body.is_none() {
        for verb in [
            "remind me that i need to ",
            "remind me that i have to ",
            "remind me that i must ",
            "remind me that i should ",
            "remind me that i've got to ",
            "remind me i need to ",
            "remind me i have to ",
            "remind me to ",
            "remind me that ",
            "remind me about ",
            "remind me of ",
            "remind me ",
        ] {
            if let Some(rest) = strip_prefix_ci(orig, verb) {
                body = Some(rest);
                break;
            }
        }
    }

    // Declarative + imperative combo: "I need to buy milk, add it to my tasks"
    if body.is_none() {
        for marker in [
            ", add it",
            ", and add it",
            ", add this",
            ", add that",
            ", add them",
            ". add it",
            ", add it to",
        ] {
            if let Some(idx) = lower.find(marker) {
                let before = orig[..idx].trim();
                for verb in [
                    "i need to ",
                    "i have to ",
                    "i must ",
                    "i should ",
                    "i want to ",
                    "i wanna ",
                    "i gotta ",
                    "gotta ",
                    "i really need to ",
                ] {
                    if let Some(rest) = strip_prefix_ci(before, verb) {
                        body = Some(rest);
                        break;
                    }
                }
                if body.is_some() {
                    break;
                }
            }
        }
    }

    // Plain need-statements: "I need to buy milk"
    if body.is_none() {
        for verb in [
            "i need to ",
            "i have to ",
            "i must ",
            "i should ",
            "i want to ",
            "i gotta ",
        ] {
            if let Some(rest) = strip_prefix_ci(orig, verb) {
                body = Some(rest);
                break;
            }
        }
    }

    let body = body?;
    // Slots are extracted from the whole body first: a due date or priority can
    // follow the list marker ("… to my to-dos due today at 6 pm"). The title is
    // then cleaned, which also strips the list marker itself.
    let slots = extract(body);
    let title = clean_add_title(&slots.title)?;

    let mut params = serde_json::Map::new();
    params.insert("title".into(), json!(title));
    if let Some(due) = slots.due {
        params.insert("due".into(), json!(due));
    }
    if let Some(priority) = slots.priority {
        params.insert("priority".into(), json!(priority));
    }
    if let Some(category) = slots.category {
        params.insert("category".into(), json!(category));
    }
    Some(CommandRequest {
        intent: "ADD_TODO".into(),
        parameters: Value::Object(params),
    })
}

fn try_open(lower: &str, orig: &str) -> Option<CommandRequest> {
    let mut rest: Option<&str> = None;
    for verb in ["open ", "launch ", "start ", "run ", "fire up ", "switch to ", "go to "] {
        if let Some(r) = strip_prefix_ci(orig, verb) {
            rest = Some(r);
            break;
        }
    }
    let rest = tidy(rest?);
    if rest.is_empty() {
        return None;
    }
    let rest_lower = rest.to_ascii_lowercase();
    let _ = lower;

    // Never a shell/script launch.
    if ["script", "shell", "powershell", "command prompt", "terminal command", "cmd "]
        .iter()
        .any(|kw| rest_lower.contains(kw))
    {
        return None;
    }

    // A request that also asks for a web search ("open chrome and search for
    // X") is not something Nova can carry out: it must reach the honest
    // "isn't supported yet" path instead of claiming to have opened it.
    if [
        "search for",
        "search the web",
        "search online",
        "search the internet",
        "search up",
        "look up",
        "google for",
    ]
    .iter()
    .any(|needle| rest_lower.contains(needle))
    {
        return None;
    }

    // Todo-list phrasing that landed here ("open my todo list") — but an app
    // whose name contains "to do" (Microsoft To Do) must still open the app.
    const TODO_APPS: &[&str] = &[
        "microsoft to do",
        "microsoft todo",
        "ms to do",
        "ms todo",
        "todoist",
        "to do app",
        "todo app",
    ];
    if TODO_APPS.iter().any(|app| rest_lower.contains(app)) {
        return Some(CommandRequest {
            intent: "OPEN_APPLICATION".into(),
            parameters: json!({ "application": rest }),
        });
    }
    let refers_to_the_list = rest_lower.contains("task")
        || rest_lower.contains("list")
        || rest_lower.contains("checklist")
        || rest_lower.ends_with("todos")
        || rest_lower == "todo"
        || rest_lower == "to do";
    if refers_to_the_list {
        return Some(CommandRequest {
            intent: "LIST_TODOS".into(),
            parameters: json!({}),
        });
    }

    // Folder?
    let folder_rest = if let Some(stripped) = rest_lower.strip_suffix(" folder") {
        Some(rest[..stripped.len()].trim().to_string())
    } else {
        None
    };
    let candidate_folder = folder_rest.clone().unwrap_or_else(|| rest.to_string());
    let candidate_lower = strip_leading_articles(&candidate_folder.to_ascii_lowercase());
    const FOLDERS: &[&str] = &[
        "downloads", "download", "documents", "document", "docs", "desktop", "pictures",
        "picture", "photos", "music", "videos", "video", "home", "home folder", "user folder",
    ];
    // "… folder" anywhere in the phrase means that folder, even in a longer
    // sentence ("open in download folder some sub folder inside download").
    if (rest_lower.contains("folder") || rest_lower.contains("directory"))
        && folder_rest.is_none()
    {
        if let Some(folder) = FOLDERS
            .iter()
            .find(|name| name.len() >= 4 && rest_lower.contains(*name))
        {
            return Some(CommandRequest {
                intent: "OPEN_FOLDER".into(),
                parameters: json!({ "folder": (*folder).to_string() }),
            });
        }
    }
    if FOLDERS.contains(&candidate_lower.as_str()) || folder_rest.is_some() {
        return Some(CommandRequest {
            intent: "OPEN_FOLDER".into(),
            parameters: json!({ "folder": candidate_folder }),
        });
    }

    // File?
    let looks_like_path =
        rest.contains('\\') || rest.contains('/') || rest_lower.starts_with("c:") || rest_lower.starts_with("d:");
    let known_file_exts = [
        "pdf", "txt", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "png", "jpg", "jpeg", "gif",
        "md", "csv", "json", "mp4", "mp3", "wav", "zip", "log",
    ];
    let has_known_ext = rest
        .rsplit('.')
        .next()
        .map(|ext| known_file_exts.contains(&ext.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
        && rest.contains('.');
    if looks_like_path || has_known_ext {
        return Some(CommandRequest {
            intent: "OPEN_FILE".into(),
            parameters: json!({ "path": rest }),
        });
    }

    // Otherwise: an application name.
    Some(CommandRequest {
        intent: "OPEN_APPLICATION".into(),
        parameters: json!({ "application": rest }),
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str) -> Option<CommandRequest> {
        interpret(text, &ConversationContext::default())
    }

    fn run_with_ctx(text: &str, last_todo_id: i64) -> Option<CommandRequest> {
        let mut ctx = ConversationContext::default();
        ctx.last_todo_id = Some(last_todo_id);
        interpret(text, &ctx)
    }

    fn expect(text: &str, intent: &str, param: &str, value: &str) {
        let cmd = run(text).unwrap_or_else(|| panic!("expected a command for: {text}"));
        assert_eq!(cmd.intent, intent, "intent mismatch for: {text}");
        assert_eq!(
            cmd.parameters.get(param).and_then(|v| v.as_str()),
            Some(value),
            "parameter mismatch for: {text} -> {:?}",
            cmd.parameters
        );
    }

    #[test]
    fn add_todo_variants_spec_26() {
        // All of these must resolve to the same ADD_TODO capability.
        expect("Add buy milk to my list", "ADD_TODO", "title", "buy milk");
        expect("Remind me that I need to buy milk", "ADD_TODO", "title", "buy milk");
        expect("Put buy milk on my todo list", "ADD_TODO", "title", "buy milk");
        expect("I need to buy milk, add it to my tasks", "ADD_TODO", "title", "buy milk");
    }

    #[test]
    fn add_todo_variants_more() {
        expect(
            "Add finish DBMS assignment to my todo list",
            "ADD_TODO",
            "title",
            "finish DBMS assignment",
        );
        expect(
            "Could you add finish operating systems assignment to my tasks please",
            "ADD_TODO",
            "title",
            "finish operating systems assignment",
        );
        expect("remind me to finish the DBMS lab", "ADD_TODO", "title", "finish the DBMS lab");
        expect("note down call mom", "ADD_TODO", "title", "call mom");
    }

    #[test]
    fn add_todo_rejects_garbage() {
        assert!(run("add it to my list").is_none());
        assert!(run("make me a sandwich").is_none());
    }

    #[test]
    fn add_todo_extracts_due_priority_and_category() {
        let cmd = run("Add finish my DBMS assignment to my to-dos due today at 6 pm").unwrap();
        assert_eq!(cmd.intent, "ADD_TODO");
        assert_eq!(
            cmd.parameters.get("title").and_then(|v| v.as_str()),
            Some("finish my DBMS assignment")
        );
        assert!(
            cmd.parameters.get("due").and_then(|v| v.as_str()).is_some(),
            "the due date should be resolved: {:?}",
            cmd.parameters
        );

        let cmd = run("Add prepare for my technical interview to my tasks high priority for career")
            .unwrap();
        assert_eq!(cmd.intent, "ADD_TODO");
        assert_eq!(
            cmd.parameters.get("title").and_then(|v| v.as_str()),
            Some("prepare for my technical interview")
        );
        assert_eq!(
            cmd.parameters.get("priority").and_then(|v| v.as_str()),
            Some("high")
        );
        assert_eq!(
            cmd.parameters.get("category").and_then(|v| v.as_str()),
            Some("Career")
        );
    }

    #[test]
    fn summarize_variants_cover_the_designed_questions() {
        let cmd = run("What do I need to finish today?").unwrap();
        assert_eq!(cmd.intent, "SUMMARIZE_TODOS");
        assert_eq!(cmd.parameters.get("scope").and_then(|v| v.as_str()), Some("today"));

        let cmd = run("What's on my list for tomorrow?").unwrap();
        assert_eq!(cmd.intent, "SUMMARIZE_TODOS");
        assert_eq!(cmd.parameters.get("scope").and_then(|v| v.as_str()), Some("tomorrow"));

        let cmd = run("what's overdue").unwrap();
        assert_eq!(cmd.parameters.get("scope").and_then(|v| v.as_str()), Some("overdue"));

        // A plain list request stays a list request.
        assert_eq!(run("what is on my list").unwrap().intent, "LIST_TODOS");
        assert_eq!(run("What's on my todo list?").unwrap().intent, "LIST_TODOS");
    }

    #[test]
    fn edit_variants() {
        let cmd = run("rename the DBMS assignment to finish the DBMS lab").unwrap();
        assert_eq!(cmd.intent, "EDIT_TODO");
        assert_eq!(
            cmd.parameters.get("searchQuery").and_then(|v| v.as_str()),
            Some("DBMS assignment")
        );
        assert_eq!(
            cmd.parameters.get("title").and_then(|v| v.as_str()),
            Some("finish the DBMS lab")
        );

        let cmd = run("set the interview task to high priority").unwrap();
        assert_eq!(cmd.intent, "EDIT_TODO");
        assert_eq!(
            cmd.parameters.get("priority").and_then(|v| v.as_str()),
            Some("high")
        );
        assert_eq!(
            cmd.parameters.get("searchQuery").and_then(|v| v.as_str()),
            Some("interview")
        );

        let cmd = run("move the essay task to tomorrow").unwrap();
        assert_eq!(cmd.intent, "EDIT_TODO");
        assert!(cmd.parameters.get("due").is_some(), "{:?}", cmd.parameters);

        let cmd = run("set the due date of the report to next monday").unwrap();
        assert_eq!(cmd.intent, "EDIT_TODO");
        assert!(cmd.parameters.get("due").is_some(), "{:?}", cmd.parameters);

        let cmd = run("clear the due date on the essay task").unwrap();
        assert_eq!(cmd.intent, "EDIT_TODO");
        assert_eq!(
            cmd.parameters.get("clearDue").and_then(|v| v.as_bool()),
            Some(true)
        );

        // Ordinary commands are untouched by the edit patterns.
        assert_eq!(run("complete buy milk").unwrap().intent, "COMPLETE_TODO");
        assert_eq!(run("delete the milk todo").unwrap().intent, "DELETE_TODO");
    }

    #[test]
    fn list_variants() {
        for text in [
            "What's on my todo list?",
            "Show my todos",
            "List my tasks",
            "what is on my list",
            "show me my checklist",
        ] {
            let cmd = run(text).unwrap_or_else(|| panic!("expected LIST for: {text}"));
            assert_eq!(cmd.intent, "LIST_TODOS", "text: {text}");
        }
    }

    #[test]
    fn complete_variants() {
        expect(
            "Mark DBMS assignment as completed",
            "COMPLETE_TODO",
            "searchQuery",
            "DBMS assignment",
        );
        expect(
            "finish the DBMS lab on my list",
            "COMPLETE_TODO",
            "searchQuery",
            "DBMS lab",
        );
        expect("complete buy milk", "COMPLETE_TODO", "searchQuery", "buy milk");
        expect("mark the milk task as done", "COMPLETE_TODO", "searchQuery", "milk");
    }

    #[test]
    fn complete_pronoun_uses_context() {
        let cmd = run_with_ctx("complete it", 42).unwrap();
        assert_eq!(cmd.intent, "COMPLETE_TODO");
        assert_eq!(cmd.parameters.get("todoId").and_then(|v| v.as_i64()), Some(42));
        // Without context there is nothing to complete.
        assert!(run("complete it").is_none());
    }

    #[test]
    fn delete_variants_and_guards() {
        expect("delete the milk todo", "DELETE_TODO", "searchQuery", "milk");
        expect(
            "remove buy milk from my list",
            "DELETE_TODO",
            "searchQuery",
            "buy milk",
        );
        // File/folder deletion must never be misrouted to the todo skill.
        assert!(run("delete the downloads folder").is_none());
        assert!(run("remove all files from documents").is_none());
    }

    #[test]
    fn clear_completed_variants() {
        for text in [
            "clear my completed todos",
            "remove all finished tasks",
            "delete completed items from my list",
        ] {
            let cmd = run(text).unwrap_or_else(|| panic!("expected CLEAR for: {text}"));
            assert_eq!(cmd.intent, "CLEAR_COMPLETED_TODOS", "text: {text}");
        }
    }

    #[test]
    fn search_variants() {
        expect("search my todos for DBMS", "SEARCH_TODOS", "query", "DBMS");
        expect("find milk in my list", "SEARCH_TODOS", "query", "milk");
        expect("look for assignment in my tasks", "SEARCH_TODOS", "query", "assignment");
    }

    #[test]
    fn open_app_variants() {
        expect("open Spotify", "OPEN_APPLICATION", "application", "Spotify");
        expect("launch notepad", "OPEN_APPLICATION", "application", "notepad");
        expect("start the calculator", "OPEN_APPLICATION", "application", "the calculator");
    }

    #[test]
    fn open_folder_variants() {
        expect("open my Downloads folder", "OPEN_FOLDER", "folder", "my Downloads");
        expect("open the documents folder", "OPEN_FOLDER", "folder", "the documents");
        expect("open downloads", "OPEN_FOLDER", "folder", "downloads");
    }

    #[test]
    fn open_file_variants() {
        expect("open report.pdf", "OPEN_FILE", "path", "report.pdf");
        let cmd = run("open C:\\Users\\Haris\\Desktop\\notes.txt").unwrap();
        assert_eq!(cmd.intent, "OPEN_FILE");
    }

    #[test]
    fn screenshot_variants() {
        for text in ["take a screenshot", "capture my screen", "screenshot please"] {
            let cmd = run(text).unwrap_or_else(|| panic!("expected SCREENSHOT for: {text}"));
            assert_eq!(cmd.intent, "TAKE_SCREENSHOT", "text: {text}");
        }
    }

    #[test]
    fn delete_accepts_a_named_target_without_the_word_todo() {
        // Regression: "delete the DBMS assignment" was refused while
        // "delete the DBMS todo" worked.
        let cmd = run("delete the DBMS assignment").unwrap();
        assert_eq!(cmd.intent, "DELETE_TODO");
        assert_eq!(
            cmd.parameters.get("searchQuery").and_then(|v| v.as_str()),
            Some("DBMS assignment")
        );
        // File deletion is still refused outright.
        assert!(run("delete the report file in downloads").is_none());
    }

    #[test]
    fn open_requests_with_a_web_search_are_refused() {
        // Regression: "open Google Chrome and search for the most best supercar"
        // was reported as opened even though Nova cannot search the web.
        assert!(run("Can you open google chrome and search for the most best supercar?").is_none());
        assert!(run("open chrome and look up supercars").is_none());
        // Opening an app on its own still works — including one whose name
        // starts with a search engine's.
        assert_eq!(run("open google chrome").unwrap().intent, "OPEN_APPLICATION");
        assert_eq!(run("open google chrome").unwrap().parameters["application"], "google chrome");
    }

    #[test]
    fn folders_are_recognised_inside_longer_phrases() {
        let cmd = run("Can you open in download folder some random sub folder inside download")
            .expect("expected a folder request");
        assert_eq!(cmd.intent, "OPEN_FOLDER");
        assert_eq!(cmd.parameters.get("folder").and_then(|v| v.as_str()), Some("download"));
        assert_eq!(
            run("open my documents folder").unwrap().intent,
            "OPEN_FOLDER"
        );
    }

    #[test]
    fn opening_the_todo_app_is_not_a_list_request() {
        // Regression: "open Microsoft to do" used to answer with the task list.
        let cmd = run("Can you open Microsoft to do?").unwrap();
        assert_eq!(cmd.intent, "OPEN_APPLICATION");
        assert_eq!(
            cmd.parameters.get("application").and_then(|v| v.as_str()),
            Some("Microsoft to do")
        );
        assert_eq!(run("open todoist").unwrap().intent, "OPEN_APPLICATION");

        // The list phrasings still list tasks.
        for text in ["open my todo list", "open my todos", "open my tasks"] {
            assert_eq!(
                run(text).unwrap().intent,
                "LIST_TODOS",
                "expected a list request for: {text}"
            );
        }
    }

    #[test]
    fn shutdown_phrasings_require_the_machine_to_be_named() {
        for text in [
            "shut down my PC",
            "shut down the computer",
            "power off the laptop",
            "turn off the computer please",
        ] {
            let cmd = run(text).unwrap_or_else(|| panic!("expected SHUTDOWN_PC for: {text}"));
            assert_eq!(cmd.intent, "SHUTDOWN_PC", "text: {text}");
        }

        // Nova itself is never shut down, and vague phrases are refused.
        assert!(run("shut down Nova").is_none());
        assert!(run("shut down").is_none());

        let cancel = run("cancel the shutdown").unwrap();
        assert_eq!(cancel.intent, "CANCEL_SHUTDOWN");
    }

    #[test]
    fn web_search_requests_are_not_task_searches() {
        // They must reach the "not supported yet" path, not a task lookup.
        for text in [
            "search the web for DBMS notes",
            "google the best laptops",
            "search online for interview questions",
        ] {
            assert!(run(text).is_none(), "expected no match for: {text}");
        }
        // A genuine task search still works.
        expect("search my todos for DBMS", "SEARCH_TODOS", "query", "DBMS");
    }

    #[test]
    fn no_match_cases() {
        for text in [
            "make me a sandwich",
            "what is the meaning of life",
            "play some music",
            "book a flight to paris",
        ] {
            assert!(run(text).is_none(), "expected no match for: {text}");
        }
    }

    #[test]
    fn script_and_shell_requests_are_not_matched() {
        assert!(run("open powershell").is_none());
        assert!(run("run a script").is_none());
        assert!(run("start command prompt").is_none());
    }
}
