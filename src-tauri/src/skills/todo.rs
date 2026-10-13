//! Task skill: every list operation exposed to the agent and the dashboard.
//! The executor resolves ambiguous references safely — the model never gets
//! to manipulate the database directly.

use chrono::{DateTime, Duration, Local, NaiveTime, TimeZone, Utc};
use rusqlite::Connection;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager, Runtime};

use crate::commands::SkillOutcome;
use crate::database::{self, NewTodo, Todo, TodoPatch};
use crate::error::{NovaError, NovaResult};
use crate::state::AppState;
use crate::util::{title_similarity, truncate_for_speech};

/// Outcome of resolving a fuzzy task reference.
pub enum ResolvedTodo {
    Unique(Todo),
    /// Several plausible matches — we must ask before mutating anything.
    Ambiguous(Vec<Todo>),
    None,
}

/// Resolve a task by explicit id or fuzzy search query.
pub fn resolve_todo(
    conn: &Connection,
    search: Option<&str>,
    id: Option<i64>,
) -> NovaResult<ResolvedTodo> {
    if let Some(id) = id {
        return Ok(match database::get_todo(conn, id)? {
            Some(todo) => ResolvedTodo::Unique(todo),
            None => ResolvedTodo::None,
        });
    }

    let Some(query) = search.map(str::trim).filter(|q| !q.is_empty()) else {
        return Ok(ResolvedTodo::None);
    };

    // 1) Direct substring matches.
    let mut candidates = database::search_todos(conn, query)?;

    if candidates.is_empty() {
        // 2) Fuzzy fallback over open tasks (then all tasks).
        let mut pool = database::list_todos(conn, Some(false))?;
        if pool.is_empty() {
            pool = database::list_todos(conn, None)?;
        }
        let mut scored: Vec<(f32, Todo)> = pool
            .into_iter()
            .map(|todo| (title_similarity(query, &todo.title), todo))
            .collect();
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        let top = scored.first().map(|(s, _)| *s).unwrap_or(0.0);
        if top < 0.55 {
            return Ok(ResolvedTodo::None);
        }
        let second = scored.get(1).map(|(s, _)| *s).unwrap_or(0.0);
        if top - second >= 0.15 {
            return Ok(ResolvedTodo::Unique(scored.remove(0).1));
        }
        let ambiguous = scored.into_iter().take(3).map(|(_, t)| t).collect();
        return Ok(ResolvedTodo::Ambiguous(ambiguous));
    }

    // Refine substring matches: exact title wins outright.
    if let Some(exact) = candidates
        .iter()
        .find(|t| t.title.eq_ignore_ascii_case(query))
    {
        return Ok(ResolvedTodo::Unique(exact.clone()));
    }
    if candidates.len() == 1 {
        return Ok(ResolvedTodo::Unique(candidates.remove(0)));
    }

    let mut scored: Vec<(f32, Todo)> = candidates
        .into_iter()
        .map(|todo| (title_similarity(query, &todo.title), todo))
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    let top = scored[0].0;
    let second = scored.get(1).map(|(s, _)| *s).unwrap_or(0.0);
    if top - second >= 0.12 {
        Ok(ResolvedTodo::Unique(scored.remove(0).1))
    } else {
        Ok(ResolvedTodo::Ambiguous(
            scored.into_iter().take(3).map(|(_, t)| t).collect(),
        ))
    }
}

fn ambiguous_outcome(candidates: &[Todo]) -> SkillOutcome {
    let titles: Vec<String> = candidates
        .iter()
        .map(|t| format!("'{}'", truncate_for_speech(&t.title, 60)))
        .collect();
    SkillOutcome {
        success: false,
        response: format!(
            "I found several matching tasks: {}. Which one did you mean?",
            titles.join(", ")
        ),
        data: json!({
            "ambiguous": true,
            "requiresClarification": true,
            "candidates": titles,
        }),
    }
}

fn not_found_outcome() -> SkillOutcome {
    SkillOutcome {
        success: false,
        response: "I couldn't find that task on your list.".into(),
        data: json!({ "found": false }),
    }
}

/// Human description of when a task is due, e.g. "today at 6:00 PM".
pub fn describe_due(todo: &Todo) -> Option<String> {
    let due = todo.due_at.as_ref()?;
    let parsed = DateTime::parse_from_rfc3339(due).ok()?.with_timezone(&Local);
    let today = Local::now().date_naive();
    let date = parsed.date_naive();
    let day = if date == today {
        "today".to_string()
    } else if date == today + Duration::days(1) {
        "tomorrow".to_string()
    } else if date < today {
        format!("overdue since {}", parsed.format("%b %-d"))
    } else {
        parsed.format("%a, %b %-d").to_string()
    };
    let midnight = NaiveTime::from_hms_opt(0, 0, 0).unwrap();
    if parsed.time() == midnight {
        Some(day)
    } else {
        Some(format!("{day} at {}", parsed.format("%-I:%M %p")))
    }
}

/// The kind of due-date window a summary covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummaryScope {
    Today,
    Tomorrow,
    Week,
    Overdue,
    All,
}

impl SummaryScope {
    pub fn parse(value: Option<&str>) -> SummaryScope {
        match value.unwrap_or("today").trim().to_ascii_lowercase().as_str() {
            "tomorrow" => SummaryScope::Tomorrow,
            "week" => SummaryScope::Week,
            "overdue" => SummaryScope::Overdue,
            "all" => SummaryScope::All,
            _ => SummaryScope::Today,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SummaryScope::Today => "today",
            SummaryScope::Tomorrow => "tomorrow",
            SummaryScope::Week => "this week",
            SummaryScope::Overdue => "overdue",
            SummaryScope::All => "on your list",
        }
    }

    /// Local-time window `[start, end)` this scope covers.
    fn window(self) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
        let start_of_today = Local::now()
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .expect("valid midnight");
        let local = Local;
        let start = local
            .from_local_datetime(&start_of_today)
            .single()
            .map(|dt| dt.with_timezone(&Utc));
        let start = start?;
        match self {
            SummaryScope::Today => Some((start, start + Duration::days(1))),
            SummaryScope::Tomorrow => {
                Some((start + Duration::days(1), start + Duration::days(2)))
            }
            SummaryScope::Week => Some((start, start + Duration::days(7))),
            SummaryScope::All | SummaryScope::Overdue => None,
        }
    }
}

/// Tasks matching a summary scope, plus the counts the Summary card needs.
pub struct SummaryBuckets {
    pub tasks: Vec<Todo>,
    pub pending_today: usize,
    pub overdue: usize,
    pub completed_today: usize,
}

pub fn summarize_buckets(conn: &Connection, scope: SummaryScope) -> NovaResult<SummaryBuckets> {
    let all = database::list_todos(conn, None)?;
    let now = Utc::now();
    let start_of_today = Local::now()
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .expect("valid midnight");
    let start_of_today = Local
        .from_local_datetime(&start_of_today)
        .single()
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or(now);
    let end_of_today = start_of_today + Duration::days(1);

    let pending_today = all
        .iter()
        .filter(|t| !t.completed)
        .filter(|t| match &t.due_at {
            Some(due) => DateTime::parse_from_rfc3339(due)
                .map(|dt| {
                    let dt = dt.with_timezone(&Utc);
                    dt < end_of_today
                })
                .unwrap_or(false),
            None => false,
        })
        .count();
    let overdue = all.iter().filter(|t| t.is_overdue()).count();
    let completed_today = all
        .iter()
        .filter(|t| t.completed)
        .filter(|t| {
            t.completed_at
                .as_ref()
                .and_then(|at| DateTime::parse_from_rfc3339(at).ok())
                .map(|dt| dt.with_timezone(&Local).date_naive() == Local::now().date_naive())
                .unwrap_or(false)
        })
        .count();

    let tasks: Vec<Todo> = all
        .into_iter()
        .filter(|todo| match scope {
            SummaryScope::All => !todo.completed,
            SummaryScope::Overdue => todo.is_overdue(),
            other => {
                if todo.completed {
                    return false;
                }
                let Some(due) = &todo.due_at else { return false };
                let Ok(when) = DateTime::parse_from_rfc3339(due) else {
                    return false;
                };
                let when = when.with_timezone(&Utc);
                match other.window() {
                    Some((start, end)) => when >= start && when < end,
                    None => true,
                }
            }
        })
        .collect();

    Ok(SummaryBuckets {
        tasks,
        pending_today,
        overdue,
        completed_today,
    })
}

/// A spoken/displayed summary of the requested window.
pub fn summarize_response(buckets: &SummaryBuckets, scope: SummaryScope) -> String {
    let count = buckets.tasks.len();
    if count == 0 {
        return match scope {
            SummaryScope::Today => {
                if buckets.completed_today > 0 {
                    format!(
                        "Nothing is due today — you already completed {} {}.",
                        buckets.completed_today,
                        if buckets.completed_today == 1 { "task" } else { "tasks" }
                    )
                } else {
                    "Nothing is due today.".to_string()
                }
            }
            SummaryScope::Overdue => "Nothing is overdue — you're all caught up.".to_string(),
            other => format!("Nothing is due {}.", other.label()),
        };
    }

    let titles: Vec<String> = buckets
        .tasks
        .iter()
        .take(4)
        .map(|t| {
            let due = describe_due(t);
            match due {
                Some(due) => format!("'{}' ({due})", truncate_for_speech(&t.title, 48)),
                None => format!("'{}'", truncate_for_speech(&t.title, 48)),
            }
        })
        .collect();
    let title_refs: Vec<&str> = titles.iter().map(|title| title.as_str()).collect();
    let mut response = format!(
        "You have {} {} due {}: {}",
        count,
        if count == 1 { "task" } else { "tasks" },
        scope.label(),
        join_natural(&title_refs)
    );
    if count > titles.len() {
        response.push_str(&format!(" and {} more", count - titles.len()));
    }
    if scope == SummaryScope::Today && buckets.overdue > 0 && buckets.pending_today > count {
        response.push_str(&format!(
            " — {} of them {} overdue.",
            buckets.pending_today - count,
            if buckets.pending_today - count == 1 { "is" } else { "are" }
        ));
    } else {
        response.push('.');
    }
    response
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// Add a task. `source` records whether it came from voice, the dashboard or
/// onboarding, which the Tasks list shows as a microphone mark.
pub fn add<R: Runtime>(
    app: &AppHandle<R>,
    params: &Value,
    source: &str,
) -> NovaResult<SkillOutcome> {
    let title = params
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let description = params
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let priority = params
        .get("priority")
        .and_then(|v| v.as_str())
        .unwrap_or("medium")
        .to_string();
    let category = params
        .get("category")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let due_at = params
        .get("due")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    if title.is_empty() {
        return Err(NovaError::InvalidCommand("a task needs a title".into()));
    }

    let state = app.state::<AppState>();
    let conn = state.db.lock().expect("db lock poisoned");
    let todo = database::insert_todo_full(
        &conn,
        NewTodo {
            title,
            description,
            priority,
            category,
            due_at,
            source: source.to_string(),
        },
    )?;

    let due_note = describe_due(&todo)
        .map(|due| format!(" — due {due}"))
        .unwrap_or_default();
    Ok(SkillOutcome {
        success: true,
        response: format!(
            "Added '{}' to your list{}.",
            truncate_for_speech(&todo.title, 80),
            due_note
        ),
        data: json!({
            "todoId": todo.id,
            "title": todo.title,
            "due": todo.due_at,
            "priority": todo.priority,
            "category": todo.category,
            "source": todo.source,
        }),
    })
}

pub fn list<R: Runtime>(app: &AppHandle<R>, _params: &Value) -> NovaResult<SkillOutcome> {
    let state = app.state::<AppState>();
    let conn = state.db.lock().expect("db lock poisoned");
    let open = database::list_todos(&conn, Some(false))?;
    let completed_count = database::list_todos(&conn, Some(true))?.len();

    let response = if open.is_empty() {
        if completed_count > 0 {
            format!(
                "You have no open tasks. {} completed {} on the list.",
                completed_count,
                if completed_count == 1 { "task is" } else { "tasks are" }
            )
        } else {
            "Your list is empty.".to_string()
        }
    } else {
        let count = open.len();
        let shown: Vec<String> = open
            .iter()
            .take(6)
            .map(|t| match describe_due(t) {
                Some(due) if t.is_overdue() => format!("'{}' ({due})", t.title),
                Some(due) => format!("'{}' (due {due})", t.title),
                None => format!("'{}'", t.title),
            })
            .collect();
        let refs: Vec<&str> = shown.iter().map(|s| s.as_str()).collect();
        if count <= 6 {
            format!(
                "You have {} {}: {}.",
                num_word(count),
                if count == 1 { "open task" } else { "open tasks" },
                join_natural(&refs)
            )
        } else {
            format!(
                "You have {} open tasks, including {} and {} more.",
                count,
                join_natural(&refs[..4]),
                count - 4
            )
        }
    };

    Ok(SkillOutcome {
        success: true,
        response,
        data: json!({ "count": open.len(), "todos": open }),
    })
}

pub fn summarize<R: Runtime>(app: &AppHandle<R>, params: &Value) -> NovaResult<SkillOutcome> {
    let scope = SummaryScope::parse(params.get("scope").and_then(|v| v.as_str()));
    let state = app.state::<AppState>();
    let conn = state.db.lock().expect("db lock poisoned");
    let buckets = summarize_buckets(&conn, scope)?;
    let response = summarize_response(&buckets, scope);
    Ok(SkillOutcome {
        success: true,
        response,
        data: json!({
            "scope": scope.label(),
            "count": buckets.tasks.len(),
            "pendingToday": buckets.pending_today,
            "overdue": buckets.overdue,
            "completedToday": buckets.completed_today,
            "todos": buckets.tasks,
        }),
    })
}

pub fn complete<R: Runtime>(app: &AppHandle<R>, params: &Value) -> NovaResult<SkillOutcome> {
    let search = params.get("searchQuery").and_then(|v| v.as_str());
    let id = params.get("todoId").and_then(|v| v.as_i64());

    let state = app.state::<AppState>();
    let conn = state.db.lock().expect("db lock poisoned");

    match resolve_todo(&conn, search, id)? {
        ResolvedTodo::Unique(todo) => {
            if todo.completed {
                return Ok(SkillOutcome {
                    success: true,
                    response: format!(
                        "'{}' is already completed.",
                        truncate_for_speech(&todo.title, 60)
                    ),
                    data: json!({ "todoId": todo.id, "alreadyCompleted": true }),
                });
            }
            let updated = database::set_todo_completed(&conn, todo.id, true)?
                .ok_or_else(|| NovaError::Other("task disappeared during update".into()))?;
            Ok(SkillOutcome {
                success: true,
                response: format!(
                    "Marked '{}' as completed.",
                    truncate_for_speech(&updated.title, 60)
                ),
                data: json!({ "todoId": updated.id, "title": updated.title, "completed": true }),
            })
        }
        ResolvedTodo::Ambiguous(candidates) => Ok(ambiguous_outcome(&candidates)),
        ResolvedTodo::None => Ok(not_found_outcome()),
    }
}

/// Edit an existing task (title, due date, priority, category).
pub fn edit<R: Runtime>(app: &AppHandle<R>, params: &Value) -> NovaResult<SkillOutcome> {
    let search = params.get("searchQuery").and_then(|v| v.as_str());
    let id = params.get("todoId").and_then(|v| v.as_i64());

    let state = app.state::<AppState>();
    let conn = state.db.lock().expect("db lock poisoned");

    let todo = match resolve_todo(&conn, search, id)? {
        ResolvedTodo::Unique(todo) => todo,
        ResolvedTodo::Ambiguous(candidates) => return Ok(ambiguous_outcome(&candidates)),
        ResolvedTodo::None => return Ok(not_found_outcome()),
    };

    let patch = TodoPatch {
        title: params.get("title").and_then(|v| v.as_str()).map(String::from),
        description: params
            .get("description")
            .and_then(|v| v.as_str())
            .map(String::from),
        priority: params
            .get("priority")
            .and_then(|v| v.as_str())
            .map(String::from),
        category: params
            .get("category")
            .and_then(|v| v.as_str())
            .map(String::from),
        due_at: params.get("due").and_then(|v| v.as_str()).map(String::from),
        clear_due: params
            .get("clearDue")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
    };

    let updated = database::update_todo_full(&conn, todo.id, patch)?
        .ok_or_else(|| NovaError::Other("task disappeared during update".into()))?;

    let due_note = describe_due(&updated)
        .map(|due| format!(", due {due}"))
        .unwrap_or_default();
    Ok(SkillOutcome {
        success: true,
        response: format!(
            "Updated '{}'{}.",
            truncate_for_speech(&updated.title, 60),
            due_note
        ),
        data: json!({
            "todoId": updated.id,
            "title": updated.title,
            "due": updated.due_at,
            "priority": updated.priority,
            "category": updated.category,
        }),
    })
}

pub fn delete<R: Runtime>(app: &AppHandle<R>, params: &Value) -> NovaResult<SkillOutcome> {
    let search = params.get("searchQuery").and_then(|v| v.as_str());
    let id = params.get("todoId").and_then(|v| v.as_i64());

    let state = app.state::<AppState>();
    let conn = state.db.lock().expect("db lock poisoned");

    match resolve_todo(&conn, search, id)? {
        ResolvedTodo::Unique(todo) => {
            database::delete_todo(&conn, todo.id)?;
            Ok(SkillOutcome {
                success: true,
                response: format!(
                    "Deleted '{}' from your list.",
                    truncate_for_speech(&todo.title, 60)
                ),
                data: json!({ "todoId": todo.id, "title": todo.title, "deleted": true }),
            })
        }
        ResolvedTodo::Ambiguous(candidates) => Ok(ambiguous_outcome(&candidates)),
        ResolvedTodo::None => Ok(not_found_outcome()),
    }
}

pub fn clear_completed<R: Runtime>(app: &AppHandle<R>, _params: &Value) -> NovaResult<SkillOutcome> {
    let state = app.state::<AppState>();
    let conn = state.db.lock().expect("db lock poisoned");
    let completed = database::list_todos(&conn, Some(true))?.len();

    if completed == 0 {
        return Ok(SkillOutcome {
            success: true,
            response: "There are no completed tasks to clear.".into(),
            data: json!({ "cleared": 0 }),
        });
    }
    let cleared = database::clear_completed_todos(&conn)?;
    Ok(SkillOutcome {
        success: true,
        response: format!(
            "Cleared {} completed {} from your list.",
            cleared,
            if cleared == 1 { "task" } else { "tasks" }
        ),
        data: json!({ "cleared": cleared }),
    })
}

pub fn search<R: Runtime>(app: &AppHandle<R>, params: &Value) -> NovaResult<SkillOutcome> {
    let query = params
        .get("query")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if query.is_empty() {
        return Err(NovaError::InvalidCommand("search needs a query".into()));
    }

    let state = app.state::<AppState>();
    let conn = state.db.lock().expect("db lock poisoned");
    let mut matches = database::search_todos(&conn, &query)?;
    if matches.is_empty() {
        // Fall back to fuzzy matching so "milk task" finds "buy milk".
        let all = database::list_todos(&conn, None)?;
        let mut scored: Vec<(f32, database::Todo)> = all
            .into_iter()
            .map(|t| (title_similarity(&query, &t.title), t))
            .filter(|(s, _)| *s >= 0.55)
            .collect();
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        matches = scored.into_iter().map(|(_, t)| t).collect();
    }

    let response = if matches.is_empty() {
        format!("No tasks match '{}'.", truncate_for_speech(&query, 40))
    } else {
        let shown: Vec<String> = matches
            .iter()
            .take(5)
            .map(|t| {
                if t.completed {
                    format!("'{}' (completed)", t.title)
                } else {
                    format!("'{}'", t.title)
                }
            })
            .collect();
        let refs: Vec<&str> = shown.iter().map(|s| s.as_str()).collect();
        format!(
            "Found {} {} matching '{}': {}.",
            matches.len(),
            if matches.len() == 1 { "task" } else { "tasks" },
            truncate_for_speech(&query, 40),
            join_natural(&refs)
        )
    };

    Ok(SkillOutcome {
        success: true,
        response,
        data: json!({ "count": matches.len(), "todos": matches, "query": query }),
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn join_natural(items: &[&str]) -> String {
    match items.len() {
        0 => String::new(),
        1 => items[0].to_string(),
        2 => format!("{} and {}", items[0], items[1]),
        _ => {
            let head = items[..items.len() - 1].join(", ");
            format!("{}, and {}", head, items[items.len() - 1])
        }
    }
}

fn num_word(n: usize) -> String {
    const WORDS: [&str; 13] = [
        "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
        "eleven", "twelve",
    ];
    WORDS.get(n).map(|s| s.to_string()).unwrap_or_else(|| n.to_string())
}

/// Exposed for the dashboard: "today" counts for the Home page.
pub fn today_counts(conn: &Connection) -> NovaResult<(usize, usize, usize)> {
    let buckets = summarize_buckets(conn, SummaryScope::Today)?;
    Ok((
        buckets.pending_today,
        buckets.overdue,
        buckets.completed_today,
    ))
}

/// Weekday name helper used in tests and diagnostics.
pub fn weekday_name(date: chrono::NaiveDate) -> String {
    date.format("%a").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_unique_substring() {
        let conn = database::open_in_memory().unwrap();
        database::insert_todo(&conn, "finish DBMS assignment", "").unwrap();

        let resolved = resolve_todo(&conn, Some("dbms"), None).unwrap();
        match resolved {
            ResolvedTodo::Unique(todo) => assert_eq!(todo.title, "finish DBMS assignment"),
            _ => panic!("expected a unique match"),
        }
    }

    #[test]
    fn resolves_fuzzy_when_not_substring() {
        let conn = database::open_in_memory().unwrap();
        database::insert_todo(&conn, "buy milk", "").unwrap();

        let resolved = resolve_todo(&conn, Some("the milk thing"), None).unwrap();
        match resolved {
            ResolvedTodo::Unique(todo) => assert_eq!(todo.title, "buy milk"),
            other => panic!(
                "expected fuzzy match, got {}",
                match other {
                    ResolvedTodo::None => "none",
                    ResolvedTodo::Ambiguous(_) => "ambiguous",
                    _ => "?",
                }
            ),
        }
    }

    #[test]
    fn ambiguous_when_two_match_equally() {
        let conn = database::open_in_memory().unwrap();
        database::insert_todo(&conn, "buy milk", "").unwrap();
        database::insert_todo(&conn, "buy bread", "").unwrap();

        let resolved = resolve_todo(&conn, Some("buy"), None).unwrap();
        match resolved {
            ResolvedTodo::Ambiguous(candidates) => {
                assert!(candidates.len() >= 2);
                let outcome = ambiguous_outcome(&candidates);
                assert!(!outcome.success);
                assert_eq!(
                    outcome.data.get("requiresClarification").and_then(|v| v.as_bool()),
                    Some(true)
                );
            }
            _ => panic!("expected ambiguity"),
        }
    }

    #[test]
    fn none_when_no_match() {
        let conn = database::open_in_memory().unwrap();
        database::insert_todo(&conn, "buy milk", "").unwrap();
        let resolved = resolve_todo(&conn, Some("call the bank"), None).unwrap();
        assert!(matches!(resolved, ResolvedTodo::None));
    }

    #[test]
    fn due_descriptions_read_naturally() {
        let now = Local::now();
        let today_six = now
            .date_naive()
            .and_hms_opt(18, 0, 0)
            .unwrap()
            .and_local_timezone(Local)
            .single()
            .unwrap()
            .with_timezone(&Utc);
        let conn = database::open_in_memory().unwrap();
        let todo = database::insert_todo_full(
            &conn,
            NewTodo {
                title: "Finish DBMS assignment".into(),
                due_at: Some(today_six.to_rfc3339()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(describe_due(&todo).as_deref(), Some("today at 6:00 PM"));

        let yesterday = (now - Duration::days(1)).date_naive().and_hms_opt(0, 0, 0).unwrap()
            .and_local_timezone(Local)
            .single()
            .unwrap()
            .with_timezone(&Utc);
        let overdue = database::insert_todo_full(
            &conn,
            NewTodo {
                title: "Book dentist".into(),
                due_at: Some(yesterday.to_rfc3339()),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(overdue.is_overdue());
        assert!(describe_due(&overdue).unwrap().contains("overdue since"));
        let _ = weekday_name(now.date_naive());
    }

    #[test]
    fn summary_scopes_cover_the_designed_questions() {
        let conn = database::open_in_memory().unwrap();
        let today = Local::now()
            .date_naive()
            .and_hms_opt(18, 0, 0)
            .unwrap()
            .and_local_timezone(Local)
            .single()
            .unwrap()
            .with_timezone(&Utc);
        let tomorrow = today + Duration::days(1);
        let next_week = today + Duration::days(20);

        database::insert_todo_full(
            &conn,
            NewTodo {
                title: "Finish DBMS assignment".into(),
                due_at: Some(today.to_rfc3339()),
                ..Default::default()
            },
        )
        .unwrap();
        database::insert_todo_full(
            &conn,
            NewTodo {
                title: "Revise operating systems".into(),
                due_at: Some(tomorrow.to_rfc3339()),
                ..Default::default()
            },
        )
        .unwrap();
        database::insert_todo_full(
            &conn,
            NewTodo {
                title: "Prepare for interview".into(),
                due_at: Some(next_week.to_rfc3339()),
                ..Default::default()
            },
        )
        .unwrap();
        database::insert_todo_full(
            &conn,
            NewTodo {
                title: "Book dentist".into(),
                due_at: Some((today - Duration::days(1)).to_rfc3339()),
                ..Default::default()
            },
        )
        .unwrap();

        let today_summary = summarize_buckets(&conn, SummaryScope::Today).unwrap();
        assert_eq!(today_summary.tasks.len(), 1);
        assert!(today_summary.overdue >= 1);

        let tomorrow_summary = summarize_buckets(&conn, SummaryScope::Tomorrow).unwrap();
        assert_eq!(tomorrow_summary.tasks.len(), 1);
        assert!(summarize_response(&tomorrow_summary, SummaryScope::Tomorrow)
            .contains("Revise operating systems"));

        let overdue_summary = summarize_buckets(&conn, SummaryScope::Overdue).unwrap();
        // A task due earlier today is legitimately both "today" and "overdue",
        // so the exact count depends on the wall clock: the dentist task must
        // always be there, and nothing else may be missed.
        assert!(overdue_summary.tasks.len() >= 1);
        assert!(overdue_summary
            .tasks
            .iter()
            .any(|task| task.title.contains("dentist")));
        assert!(summarize_response(&overdue_summary, SummaryScope::Overdue).contains("dentist"));

        let all = summarize_buckets(&conn, SummaryScope::All).unwrap();
        assert_eq!(all.tasks.len(), 4);
    }

    #[test]
    fn empty_summaries_are_helpful() {
        let conn = database::open_in_memory().unwrap();
        let buckets = summarize_buckets(&conn, SummaryScope::Today).unwrap();
        assert_eq!(
            summarize_response(&buckets, SummaryScope::Today),
            "Nothing is due today."
        );
        let buckets = summarize_buckets(&conn, SummaryScope::Overdue).unwrap();
        assert!(summarize_response(&buckets, SummaryScope::Overdue).contains("caught up"));
    }
}
