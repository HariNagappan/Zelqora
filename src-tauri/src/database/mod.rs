//! SQLite access layer: connection management, embedded migrations, and the
//! queries for tasks, the activity log and persisted settings.
//!
//! Everything the UI needs is a single query away — the frontend never keeps a
//! second source of truth for persisted data.

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

use crate::error::{NovaError, NovaResult};
use crate::settings::Settings;

/// Embedded migrations, applied in order under `PRAGMA user_version`.
const MIGRATIONS: &[&str] = &[
    include_str!("../../../migrations/0001_init.sql"),
    include_str!("../../../migrations/0002_tasks_appearance.sql"),
];

/// Default database location: `<user data>/Nova/nova.db`.
pub fn default_db_path() -> NovaResult<PathBuf> {
    let dir = dirs::data_dir()
        .ok_or_else(|| NovaError::Other("could not resolve the user data directory".into()))?
        .join("Nova");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("nova.db"))
}

/// Directory Nova writes logs (and the database) to — surfaced in Settings.
pub fn data_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|dir| dir.join("Nova"))
}

/// Open (or create) the database at `path` and run migrations.
pub fn open(path: &Path) -> NovaResult<Connection> {
    let conn = Connection::open(path)?;
    // Best effort: WAL improves durability/concurrency; not fatal if unavailable.
    let _ = conn.pragma_update(None, "journal_mode", "WAL");
    conn.pragma_update(None, "foreign_keys", "ON")?;
    apply_migrations(&conn)?;
    Ok(conn)
}

/// Open an in-memory database with migrations applied (used by tests).
pub fn open_in_memory() -> NovaResult<Connection> {
    let conn = Connection::open_in_memory()?;
    apply_migrations(&conn)?;
    Ok(conn)
}

/// Apply every migration newer than the database's `user_version`.
pub fn apply_migrations(conn: &Connection) -> NovaResult<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    for (idx, sql) in MIGRATIONS.iter().enumerate() {
        let version = (idx + 1) as i64;
        if version > current {
            conn.execute_batch(sql)?;
            conn.pragma_update(None, "user_version", version)?;
        }
    }
    Ok(())
}

/// Schema version currently applied (used by diagnostics).
pub fn schema_version(conn: &Connection) -> NovaResult<i64> {
    Ok(conn.query_row("PRAGMA user_version", [], |row| row.get(0))?)
}

/// Current local time as an RFC 3339 string (used for all persisted timestamps).
pub fn now_rfc3339() -> String {
    chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, false)
}

/// Current UTC time as RFC 3339 (used for due dates so comparisons sort).
pub fn now_utc_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

// ---------------------------------------------------------------------------
// Tasks
// ---------------------------------------------------------------------------

pub const PRIORITIES: &[&str] = &["low", "medium", "high"];

/// Normalize a priority string; unknown values fall back to `medium`.
pub fn normalize_priority(value: &str) -> String {
    let lower = value.trim().to_ascii_lowercase();
    match lower.as_str() {
        "low" | "1" => "low".into(),
        "medium" | "med" | "normal" | "2" => "medium".into(),
        "high" | "urgent" | "3" => "high".into(),
        _ => "medium".into(),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Todo {
    pub id: i64,
    pub title: String,
    pub description: String,
    pub completed: bool,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
    pub priority: String,
    pub category: String,
    /// UTC RFC 3339 due date, or `None` when the task has no deadline.
    pub due_at: Option<String>,
    /// 'voice' | 'manual' | 'onboarding'
    pub source: String,
}

impl Todo {
    /// True when the task is open and its due date is in the past.
    pub fn is_overdue(&self) -> bool {
        if self.completed {
            return false;
        }
        let Some(due) = &self.due_at else {
            return false;
        };
        match chrono::DateTime::parse_from_rfc3339(due) {
            Ok(when) => when < chrono::Utc::now(),
            Err(_) => false,
        }
    }
}

fn map_todo(row: &Row<'_>) -> rusqlite::Result<Todo> {
    Ok(Todo {
        id: row.get("id")?,
        title: row.get("title")?,
        description: row.get("description")?,
        completed: row.get::<_, i64>("completed")? != 0,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
        completed_at: row.get("completed_at")?,
        priority: row.get("priority")?,
        category: row.get("category")?,
        due_at: row.get("due_at")?,
        source: row.get("source")?,
    })
}

/// Everything needed to create a task.
#[derive(Debug, Clone, Default)]
pub struct NewTodo {
    pub title: String,
    pub description: String,
    pub priority: String,
    pub category: String,
    pub due_at: Option<String>,
    pub source: String,
}

/// Partial update. `None` leaves a field untouched; `clear_due` removes the
/// due date explicitly (an empty due date and "no change" are different).
#[derive(Debug, Clone, Default)]
pub struct TodoPatch {
    pub title: Option<String>,
    pub description: Option<String>,
    pub priority: Option<String>,
    pub category: Option<String>,
    pub due_at: Option<String>,
    pub clear_due: bool,
}

pub fn insert_todo(conn: &Connection, title: &str, description: &str) -> NovaResult<Todo> {
    insert_todo_full(
        conn,
        NewTodo {
            title: title.to_string(),
            description: description.to_string(),
            priority: "medium".into(),
            category: String::new(),
            due_at: None,
            source: "manual".into(),
        },
    )
}

pub fn insert_todo_full(conn: &Connection, new: NewTodo) -> NovaResult<Todo> {
    let title = new.title.trim();
    if title.is_empty() {
        return Err(NovaError::InvalidCommand("a task needs a title".into()));
    }
    let now = now_rfc3339();
    let source = match new.source.as_str() {
        "voice" => "voice",
        "onboarding" => "onboarding",
        _ => "manual",
    };
    conn.execute(
        "INSERT INTO todos
            (title, description, completed, created_at, updated_at, priority, category, due_at, source)
         VALUES (?1, ?2, 0, ?3, ?3, ?4, ?5, ?6, ?7)",
        params![
            title,
            new.description.trim(),
            now,
            normalize_priority(&new.priority),
            new.category.trim(),
            new.due_at.as_deref(),
            source,
        ],
    )?;
    let id = conn.last_insert_rowid();
    get_todo(conn, id)?.ok_or_else(|| NovaError::Other("task vanished after insert".into()))
}

pub fn get_todo(conn: &Connection, id: i64) -> NovaResult<Option<Todo>> {
    Ok(conn
        .query_row("SELECT * FROM todos WHERE id = ?1", params![id], map_todo)
        .optional()?)
}

/// List todos. `completed = Some(true/false)` filters by completion state.
pub fn list_todos(conn: &Connection, completed: Option<bool>) -> NovaResult<Vec<Todo>> {
    let sql = match completed {
        Some(true) => {
            "SELECT * FROM todos WHERE completed = 1 ORDER BY completed_at DESC, id DESC"
        }
        Some(false) => "SELECT * FROM todos WHERE completed = 0 ORDER BY id DESC",
        None => "SELECT * FROM todos ORDER BY completed ASC, id DESC",
    };
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([], map_todo)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// Which slice of the task list a view asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskView {
    All,
    Pending,
    Completed,
    Overdue,
}

impl Default for TaskView {
    fn default() -> Self {
        TaskView::All
    }
}

/// Sort order for the Tasks screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskSort {
    Due,
    Priority,
    Created,
}

impl Default for TaskSort {
    fn default() -> Self {
        TaskSort::Due
    }
}

#[derive(Debug, Clone, Default)]
pub struct TaskFilter {
    pub view: TaskView,
    pub query: Option<String>,
    pub sort: TaskSort,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TaskCounts {
    pub all: i64,
    pub pending: i64,
    pub completed: i64,
    pub overdue: i64,
}

pub fn task_counts(conn: &Connection) -> NovaResult<TaskCounts> {
    let now = now_utc_rfc3339();
    let all = conn.query_row("SELECT COUNT(*) FROM todos", [], |row| row.get(0))?;
    let pending = conn.query_row(
        "SELECT COUNT(*) FROM todos WHERE completed = 0",
        [],
        |row| row.get(0),
    )?;
    let completed = conn.query_row(
        "SELECT COUNT(*) FROM todos WHERE completed = 1",
        [],
        |row| row.get(0),
    )?;
    let overdue = conn.query_row(
        "SELECT COUNT(*) FROM todos
         WHERE completed = 0 AND due_at IS NOT NULL AND due_at < ?1",
        params![now],
        |row| row.get(0),
    )?;
    Ok(TaskCounts {
        all,
        pending,
        completed,
        overdue,
    })
}

/// The Tasks screen query: view + search + sort applied in SQL.
pub fn list_tasks(conn: &Connection, filter: &TaskFilter) -> NovaResult<Vec<Todo>> {
    let now = now_utc_rfc3339();
    let mut sql = String::from("SELECT * FROM todos WHERE 1 = 1");
    match filter.view {
        TaskView::All => {}
        TaskView::Pending => sql.push_str(" AND completed = 0"),
        TaskView::Completed => sql.push_str(" AND completed = 1"),
        TaskView::Overdue => {
            sql.push_str(" AND completed = 0 AND due_at IS NOT NULL AND due_at < :now");
        }
    }
    let has_query = filter
        .query
        .as_deref()
        .map(|q| !q.trim().is_empty())
        .unwrap_or(false);
    if has_query {
        sql.push_str(
            " AND (title LIKE :pattern ESCAPE '\\' OR description LIKE :pattern ESCAPE '\\'
              OR category LIKE :pattern ESCAPE '\\')",
        );
    }
    // Open tasks first, then the requested ordering.
    match filter.sort {
        TaskSort::Due => sql.push_str(
            " ORDER BY completed ASC, due_at IS NULL ASC, due_at ASC, id DESC",
        ),
        TaskSort::Priority => sql.push_str(
            " ORDER BY completed ASC,
              CASE priority WHEN 'high' THEN 3 WHEN 'medium' THEN 2 ELSE 1 END DESC,
              due_at IS NULL ASC, due_at ASC, id DESC",
        ),
        TaskSort::Created => sql.push_str(" ORDER BY completed ASC, id DESC"),
    }

    let pattern = filter.query.as_deref().map(like_pattern);
    let mut stmt = conn.prepare(&sql)?;
    let mut bind: Vec<(&str, &dyn rusqlite::ToSql)> = Vec::new();
    if sql.contains(":now") {
        bind.push((":now", &now));
    }
    if let Some(pattern) = &pattern {
        bind.push((":pattern", pattern));
    }
    let rows = stmt.query_map(bind.as_slice(), map_todo)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

fn like_pattern(query: &str) -> String {
    format!(
        "%{}%",
        query
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    )
}

/// Case-insensitive substring search over title + description + category.
pub fn search_todos(conn: &Connection, query: &str) -> NovaResult<Vec<Todo>> {
    let pattern = like_pattern(query);
    let mut stmt = conn.prepare(
        "SELECT * FROM todos
         WHERE title LIKE ?1 ESCAPE '\\' OR description LIKE ?1 ESCAPE '\\'
            OR category LIKE ?1 ESCAPE '\\'
         ORDER BY completed ASC, id DESC",
    )?;
    let rows = stmt.query_map(params![pattern], map_todo)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn set_todo_completed(conn: &Connection, id: i64, completed: bool) -> NovaResult<Option<Todo>> {
    let now = now_rfc3339();
    let changed = if completed {
        conn.execute(
            "UPDATE todos SET completed = 1, completed_at = ?2, updated_at = ?2 WHERE id = ?1",
            params![id, now],
        )?
    } else {
        conn.execute(
            "UPDATE todos SET completed = 0, completed_at = NULL, updated_at = ?2 WHERE id = ?1",
            params![id, now],
        )?
    };
    if changed == 0 {
        return Ok(None);
    }
    get_todo(conn, id)
}

pub fn update_todo(
    conn: &Connection,
    id: i64,
    title: Option<&str>,
    description: Option<&str>,
) -> NovaResult<Option<Todo>> {
    update_todo_full(
        conn,
        id,
        TodoPatch {
            title: title.map(|s| s.to_string()),
            description: description.map(|s| s.to_string()),
            ..Default::default()
        },
    )
}

pub fn update_todo_full(
    conn: &Connection,
    id: i64,
    patch: TodoPatch,
) -> NovaResult<Option<Todo>> {
    let Some(mut todo) = get_todo(conn, id)? else {
        return Ok(None);
    };
    if let Some(title) = &patch.title {
        let trimmed = title.trim();
        if !trimmed.is_empty() {
            todo.title = trimmed.to_string();
        }
    }
    if let Some(description) = &patch.description {
        todo.description = description.clone();
    }
    if let Some(priority) = &patch.priority {
        todo.priority = normalize_priority(priority);
    }
    if let Some(category) = &patch.category {
        todo.category = category.trim().to_string();
    }
    if patch.clear_due {
        todo.due_at = None;
    } else if let Some(due) = &patch.due_at {
        if due.trim().is_empty() {
            todo.due_at = None;
        } else {
            todo.due_at = Some(due.trim().to_string());
        }
    }

    conn.execute(
        "UPDATE todos
            SET title = ?2, description = ?3, priority = ?4, category = ?5, due_at = ?6,
                updated_at = ?7
          WHERE id = ?1",
        params![
            id,
            todo.title,
            todo.description,
            todo.priority,
            todo.category,
            todo.due_at,
            now_rfc3339()
        ],
    )?;
    get_todo(conn, id)
}

pub fn delete_todo(conn: &Connection, id: i64) -> NovaResult<bool> {
    Ok(conn.execute("DELETE FROM todos WHERE id = ?1", params![id])? > 0)
}

pub fn clear_completed_todos(conn: &Connection) -> NovaResult<usize> {
    // `execute` on DELETE reports the number of removed rows.
    Ok(conn.execute("DELETE FROM todos WHERE completed = 1", [])?)
}

// ---------------------------------------------------------------------------
// Activity log
// ---------------------------------------------------------------------------

/// Status of a logged interaction (mirrors the Activity screen's badges).
pub const STATUS_SUCCESS: &str = "success";
pub const STATUS_FAILED: &str = "failed";
pub const STATUS_CANCELLED: &str = "cancelled";
pub const STATUS_AWAITING_CONFIRMATION: &str = "awaiting_confirmation";
pub const STATUS_AWAITING_CLARIFICATION: &str = "awaiting_clarification";
pub const STATUS_NOT_HEARD: &str = "not_heard";
pub const STATUS_UNSUPPORTED: &str = "unsupported";
pub const STATUS_TIMEOUT: &str = "timeout";

pub fn normalize_status(value: &str) -> &'static str {
    match value.trim().to_ascii_lowercase().as_str() {
        "success" | "ok" | "done" => STATUS_SUCCESS,
        "cancelled" | "canceled" => STATUS_CANCELLED,
        "awaiting_confirmation" | "pending" | "awaiting" => STATUS_AWAITING_CONFIRMATION,
        "awaiting_clarification" | "clarification" => STATUS_AWAITING_CLARIFICATION,
        "not_heard" | "unheard" => STATUS_NOT_HEARD,
        "unsupported" | "not_supported" => STATUS_UNSUPPORTED,
        "timeout" | "timed_out" => STATUS_TIMEOUT,
        _ => STATUS_FAILED,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ActivityEntry {
    pub id: i64,
    pub timestamp: String,
    pub transcript: String,
    pub intent: Option<String>,
    pub intent_label: Option<String>,
    pub parameters: Option<String>,
    pub result: Option<String>,
    pub success: bool,
    pub response: String,
    pub duration_ms: Option<i64>,
    pub status: String,
    pub heard: String,
    pub understood: Option<String>,
    pub action: Option<String>,
    pub retryable: bool,
}

fn map_activity(row: &Row<'_>) -> rusqlite::Result<ActivityEntry> {
    Ok(ActivityEntry {
        id: row.get("id")?,
        timestamp: row.get("timestamp")?,
        transcript: row.get("transcript")?,
        intent: row.get("intent")?,
        intent_label: row.get("intent_label")?,
        parameters: row.get("parameters")?,
        result: row.get("result")?,
        success: row.get::<_, i64>("success")? != 0,
        response: row.get("response")?,
        duration_ms: row.get("duration_ms")?,
        status: row.get("status")?,
        heard: row.get("heard")?,
        understood: row.get("understood")?,
        action: row.get("action")?,
        retryable: row.get::<_, i64>("retryable")? != 0,
    })
}

pub struct NewActivity<'a> {
    pub transcript: &'a str,
    pub intent: Option<&'a str>,
    pub parameters: Option<&'a str>,
    pub result: Option<&'a str>,
    pub success: bool,
    pub response: &'a str,
    pub duration_ms: Option<i64>,
    pub status: &'a str,
    pub intent_label: Option<&'a str>,
    pub understood: Option<&'a str>,
    pub action: Option<&'a str>,
    pub retryable: bool,
}

impl<'a> Default for NewActivity<'a> {
    fn default() -> Self {
        NewActivity {
            transcript: "",
            intent: None,
            parameters: None,
            result: None,
            success: false,
            response: "",
            duration_ms: None,
            status: STATUS_FAILED,
            intent_label: None,
            understood: None,
            action: None,
            retryable: false,
        }
    }
}

pub fn insert_activity(conn: &Connection, activity: NewActivity<'_>) -> NovaResult<i64> {
    conn.execute(
        "INSERT INTO activity_log
            (timestamp, transcript, intent, parameters, result, success, response, duration_ms,
             status, heard, understood, action, retryable, intent_label)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            now_rfc3339(),
            activity.transcript,
            activity.intent,
            activity.parameters,
            activity.result,
            if activity.success { 1 } else { 0 },
            activity.response,
            activity.duration_ms,
            normalize_status(activity.status),
            activity.transcript,
            activity.understood,
            activity.action,
            if activity.retryable { 1 } else { 0 },
            activity.intent_label,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_activity(conn: &Connection, limit: i64, offset: i64) -> NovaResult<Vec<ActivityEntry>> {
    let mut stmt = conn.prepare(
        "SELECT * FROM activity_log ORDER BY id DESC LIMIT ?1 OFFSET ?2",
    )?;
    let rows = stmt.query_map(params![limit, offset], map_activity)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

#[derive(Debug, Clone, Default)]
pub struct ActivityFilter {
    /// 'all' | 'success' | 'failed' | 'cancelled'
    pub status: Option<String>,
    pub query: Option<String>,
    pub limit: i64,
    pub offset: i64,
}

impl ActivityFilter {
    fn status_clause(&self) -> Option<&'static str> {
        match self
            .status
            .as_deref()
            .map(|s| s.trim().to_ascii_lowercase())
            .as_deref()
        {
            Some("success") => Some("success = 1"),
            Some("failed") => Some("success = 0 AND status IN ('failed', 'unsupported', 'timeout')"),
            Some("cancelled") => Some("status = 'cancelled'"),
            _ => None,
        }
    }
}

pub fn list_activity_filtered(
    conn: &Connection,
    filter: &ActivityFilter,
) -> NovaResult<Vec<ActivityEntry>> {
    let mut sql = String::from("SELECT * FROM activity_log WHERE 1 = 1");
    if let Some(clause) = filter.status_clause() {
        sql.push_str(" AND ");
        sql.push_str(clause);
    }
    let has_query = filter
        .query
        .as_deref()
        .map(|q| !q.trim().is_empty())
        .unwrap_or(false);
    if has_query {
        sql.push_str(
            " AND (transcript LIKE :pattern ESCAPE '\\' OR intent LIKE :pattern ESCAPE '\\'
              OR intent_label LIKE :pattern ESCAPE '\\' OR response LIKE :pattern ESCAPE '\\')",
        );
    }
    sql.push_str(" ORDER BY id DESC LIMIT :limit OFFSET :offset");

    let pattern = filter.query.as_deref().map(like_pattern);
    let limit = filter.limit.clamp(1, 500);
    let offset = filter.offset.max(0);
    let mut stmt = conn.prepare(&sql)?;
    let mut bind: Vec<(&str, &dyn rusqlite::ToSql)> = Vec::new();
    if let Some(pattern) = &pattern {
        bind.push((":pattern", pattern));
    }
    bind.push((":limit", &limit));
    bind.push((":offset", &offset));
    let rows = stmt.query_map(bind.as_slice(), map_activity)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn activity_status_counts(conn: &Connection) -> NovaResult<(i64, i64, i64)> {
    let total = conn.query_row("SELECT COUNT(*) FROM activity_log", [], |row| row.get(0))?;
    let success = conn.query_row(
        "SELECT COUNT(*) FROM activity_log WHERE success = 1",
        [],
        |row| row.get(0),
    )?;
    let cancelled = conn.query_row(
        "SELECT COUNT(*) FROM activity_log WHERE status = 'cancelled'",
        [],
        |row| row.get(0),
    )?;
    Ok((total, success, cancelled))
}

pub fn get_activity(conn: &Connection, id: i64) -> NovaResult<Option<ActivityEntry>> {
    Ok(conn
        .query_row(
            "SELECT * FROM activity_log WHERE id = ?1",
            params![id],
            map_activity,
        )
        .optional()?)
}

pub fn update_activity_status(
    conn: &Connection,
    id: i64,
    status: &str,
    success: bool,
    response: &str,
) -> NovaResult<bool> {
    let changed = conn.execute(
        "UPDATE activity_log SET status = ?2, success = ?3, response = ?4 WHERE id = ?1",
        params![id, normalize_status(status), if success { 1 } else { 0 }, response],
    )?;
    Ok(changed > 0)
}

pub fn clear_activity(conn: &Connection) -> NovaResult<usize> {
    Ok(conn.execute("DELETE FROM activity_log", [])?)
}

/// Drop history older than `retention_days` (0 = keep forever).
pub fn prune_activity(conn: &Connection, retention_days: i64) -> NovaResult<usize> {
    if retention_days <= 0 {
        return Ok(0);
    }
    let cutoff = (chrono::Local::now() - chrono::Duration::days(retention_days))
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, false);
    Ok(conn.execute(
        "DELETE FROM activity_log WHERE timestamp < ?1",
        params![cutoff],
    )?)
}

#[derive(Debug, Clone, Serialize)]
pub struct TodayStats {
    pub commands_executed: i64,
    pub todos_added: i64,
    pub apps_opened: i64,
    pub screenshots: i64,
}

pub fn today_stats(conn: &Connection) -> NovaResult<TodayStats> {
    let prefix = format!("{}%", chrono::Local::now().format("%Y-%m-%d"));
    let commands_executed = conn.query_row(
        "SELECT COUNT(*) FROM activity_log WHERE timestamp LIKE ?1 AND intent IS NOT NULL AND success = 1",
        params![prefix],
        |row| row.get(0),
    )?;
    let todos_added = conn.query_row(
        "SELECT COUNT(*) FROM activity_log WHERE timestamp LIKE ?1 AND intent = 'ADD_TODO' AND success = 1",
        params![prefix],
        |row| row.get(0),
    )?;
    let apps_opened = conn.query_row(
        "SELECT COUNT(*) FROM activity_log WHERE timestamp LIKE ?1 AND intent = 'OPEN_APPLICATION' AND success = 1",
        params![prefix],
        |row| row.get(0),
    )?;
    let screenshots = conn.query_row(
        "SELECT COUNT(*) FROM activity_log WHERE timestamp LIKE ?1 AND intent = 'TAKE_SCREENSHOT' AND success = 1",
        params![prefix],
        |row| row.get(0),
    )?;
    Ok(TodayStats {
        commands_executed,
        todos_added,
        apps_opened,
        screenshots,
    })
}

// ---------------------------------------------------------------------------
// Settings + generic key/value rows (appearance, onboarding)
// ---------------------------------------------------------------------------

pub fn load_settings(conn: &Connection) -> NovaResult<Settings> {
    match load_kv(conn, "settings")? {
        Some(json) => Ok(decode_settings(&json)),
        None => Ok(Settings::default()),
    }
}

pub fn save_settings(conn: &Connection, settings: &Settings) -> NovaResult<()> {
    save_kv(conn, "settings", &encode_settings(settings)?)
}

/// Field inside the persisted settings JSON that holds the provider credential.
const CREDENTIAL_FIELD: &str = "openai_api_key";

/// Serialize settings, protecting the provider credential at rest (Windows DPAPI).
fn encode_settings(settings: &Settings) -> NovaResult<String> {
    let mut value = serde_json::to_value(settings)?;
    if let Some(object) = value.as_object_mut() {
        let protected = object
            .get(CREDENTIAL_FIELD)
            .and_then(|v| v.as_str())
            .map(crate::secrets::protect)
            .unwrap_or_default();
        object.insert(
            CREDENTIAL_FIELD.to_string(),
            serde_json::Value::String(protected),
        );
    }
    Ok(serde_json::to_string(&value)?)
}

/// Parse persisted settings, unprotecting the provider credential when needed.
/// Any failure falls back to defaults — a corrupt row must never brick Nova.
fn decode_settings(raw: &str) -> Settings {
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return Settings::default();
    };
    if let Some(object) = value.as_object_mut() {
        if let Some(stored) = object.get(CREDENTIAL_FIELD).and_then(|v| v.as_str()) {
            let plain = crate::secrets::unprotect(stored);
            object.insert(
                CREDENTIAL_FIELD.to_string(),
                serde_json::Value::String(plain),
            );
        }
    }
    serde_json::from_value(value).unwrap_or_default()
}

/// Read a raw key/value row.
pub fn load_kv(conn: &Connection, key: &str) -> NovaResult<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![key],
            |row| row.get(0),
        )
        .optional()?)
}

/// Write a raw key/value row.
pub fn save_kv(conn: &Connection, key: &str, value: &str) -> NovaResult<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

pub fn delete_kv(conn: &Connection, key: &str) -> NovaResult<()> {
    conn.execute("DELETE FROM settings WHERE key = ?1", params![key])?;
    Ok(())
}

/// Deserialize a JSON value stored under `key`, falling back to `T::default()`
/// when the row is missing or unreadable (never fatal for the UI).
pub fn load_json_or_default<T>(conn: &Connection, key: &str) -> NovaResult<T>
where
    T: serde::de::DeserializeOwned + Default,
{
    match load_kv(conn, key)? {
        Some(raw) => Ok(serde_json::from_str(&raw).unwrap_or_default()),
        None => Ok(T::default()),
    }
}

/// Persist a JSON value under `key`.
pub fn save_json<T: Serialize>(conn: &Connection, key: &str, value: &T) -> NovaResult<()> {
    save_kv(conn, key, &serde_json::to_string(value)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_apply_and_todos_crud() {
        let conn = open_in_memory().expect("open in-memory db");

        let t1 = insert_todo(&conn, "finish DBMS assignment", "").unwrap();
        assert_eq!(t1.title, "finish DBMS assignment");
        assert!(!t1.completed);
        assert_eq!(t1.priority, "medium");
        assert_eq!(t1.source, "manual");
        assert!(t1.due_at.is_none());

        let t2 = insert_todo(&conn, "buy milk", "2 liters").unwrap();
        assert_eq!(t2.description, "2 liters");

        let all = list_todos(&conn, None).unwrap();
        assert_eq!(all.len(), 2);

        let done = set_todo_completed(&conn, t1.id, true).unwrap().unwrap();
        assert!(done.completed);
        assert!(done.completed_at.is_some());

        let open = list_todos(&conn, Some(false)).unwrap();
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].id, t2.id);

        let found = search_todos(&conn, "dbms").unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, t1.id);

        assert!(delete_todo(&conn, t2.id).unwrap());
        assert_eq!(list_todos(&conn, None).unwrap().len(), 1);

        let cleared = clear_completed_todos(&conn).unwrap();
        assert_eq!(cleared, 1);
        assert!(list_todos(&conn, None).unwrap().is_empty());
    }

    #[test]
    fn task_fields_roundtrip_and_views() {
        let conn = open_in_memory().unwrap();
        let now = chrono::Utc::now();
        let overdue = (now - chrono::Duration::hours(3)).to_rfc3339();
        let later = (now + chrono::Duration::days(2)).to_rfc3339();

        let a = insert_todo_full(
            &conn,
            NewTodo {
                title: "Finish DBMS assignment".into(),
                description: "chapters 4-6".into(),
                priority: "high".into(),
                category: "College".into(),
                due_at: Some(overdue.clone()),
                source: "voice".into(),
            },
        )
        .unwrap();
        let b = insert_todo_full(
            &conn,
            NewTodo {
                title: "Book dentist".into(),
                description: String::new(),
                priority: "low".into(),
                category: "Personal".into(),
                due_at: Some(later),
                source: "manual".into(),
            },
        )
        .unwrap();

        assert_eq!(a.priority, "high");
        assert_eq!(a.category, "College");
        assert_eq!(a.source, "voice");
        assert!(a.is_overdue());
        assert!(!b.is_overdue());

        let counts = task_counts(&conn).unwrap();
        assert_eq!(counts.all, 2);
        assert_eq!(counts.pending, 2);
        assert_eq!(counts.completed, 0);
        assert_eq!(counts.overdue, 1);

        let overdue_rows = list_tasks(
            &conn,
            &TaskFilter {
                view: TaskView::Overdue,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(overdue_rows.len(), 1);
        assert_eq!(overdue_rows[0].id, a.id);

        let by_priority = list_tasks(
            &conn,
            &TaskFilter {
                sort: TaskSort::Priority,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(by_priority[0].id, a.id, "high priority sorts first");

        let searched = list_tasks(
            &conn,
            &TaskFilter {
                query: Some("college".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(searched.len(), 1);
        assert_eq!(searched[0].id, a.id, "category participates in search");

        let updated = update_todo_full(
            &conn,
            a.id,
            TodoPatch {
                priority: Some("low".into()),
                ..Default::default()
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(updated.priority, "low");
        assert_eq!(updated.due_at.as_deref(), Some(overdue.as_str()));

        let cleared = update_todo_full(
            &conn,
            a.id,
            TodoPatch {
                clear_due: true,
                ..Default::default()
            },
        )
        .unwrap()
        .unwrap();
        assert!(cleared.due_at.is_none());
        assert_eq!(task_counts(&conn).unwrap().overdue, 0);
    }

    #[test]
    fn migration_0002_preserves_existing_rows() {
        // Simulate a database created before migration 0002 existed.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(MIGRATIONS[0]).unwrap();
        conn.pragma_update(None, "user_version", 1).unwrap();
        conn.execute(
            "INSERT INTO todos (title, description, completed, created_at, updated_at)
             VALUES ('legacy task', 'kept', 0, '2025-01-01T00:00:00Z', '2025-01-01T00:00:00Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO activity_log (timestamp, transcript, success, response)
             VALUES ('2025-01-01T00:00:00Z', 'add legacy', 1, 'ok')",
            [],
        )
        .unwrap();

        apply_migrations(&conn).unwrap();

        assert_eq!(schema_version(&conn).unwrap(), 2);
        assert_eq!(list_todos(&conn, None).unwrap().len(), 1, "row preserved");
        let todo = &list_todos(&conn, None).unwrap()[0];
        assert_eq!(todo.title, "legacy task");
        assert_eq!(todo.priority, "medium", "new column defaulted");
        assert_eq!(todo.source, "manual");
        let activity = &list_activity(&conn, 10, 0).unwrap()[0];
        assert_eq!(activity.status, STATUS_SUCCESS);
        assert_eq!(activity.heard, "add legacy");
    }

    #[test]
    fn activity_and_stats() {
        let conn = open_in_memory().unwrap();
        insert_activity(
            &conn,
            NewActivity {
                transcript: "add buy milk",
                intent: Some("ADD_TODO"),
                intent_label: Some("Add task"),
                parameters: Some("{\"title\":\"buy milk\"}"),
                result: Some("{\"todoId\":1}"),
                success: true,
                response: "Added buy milk to your todo list.",
                duration_ms: Some(42),
                status: STATUS_SUCCESS,
                understood: Some("Add task · “buy milk”".into()),
                action: Some("Created the task".into()),
                retryable: false,
            },
        )
        .unwrap();
        insert_activity(
            &conn,
            NewActivity {
                transcript: "delete library task",
                intent: Some("DELETE_TODO"),
                success: false,
                response: "Okay, cancelled.",
                status: STATUS_CANCELLED,
                retryable: false,
                ..Default::default()
            },
        )
        .unwrap();

        let entries = list_activity(&conn, 10, 0).unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries[0].success == false);
        assert_eq!(entries[0].status, STATUS_CANCELLED);
        assert!(entries[1].success);
        assert_eq!(entries[1].intent_label.as_deref(), Some("Add task"));

        let filtered = list_activity_filtered(
            &conn,
            &ActivityFilter {
                status: Some("success".into()),
                limit: 20,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].intent.as_deref(), Some("ADD_TODO"));

        let cancelled = list_activity_filtered(
            &conn,
            &ActivityFilter {
                status: Some("cancelled".into()),
                limit: 20,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(cancelled.len(), 1);

        let searched = list_activity_filtered(
            &conn,
            &ActivityFilter {
                query: Some("milk".into()),
                limit: 20,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(searched.len(), 1);

        let (total, success, cancelled_count) = activity_status_counts(&conn).unwrap();
        assert_eq!((total, success, cancelled_count), (2, 1, 1));

        let stats = today_stats(&conn).unwrap();
        assert_eq!(stats.commands_executed, 1);
        assert_eq!(stats.todos_added, 1);

        assert_eq!(clear_activity(&conn).unwrap(), 2);
        assert!(list_activity(&conn, 10, 0).unwrap().is_empty());
    }

    /// A stand-in provider value used only to prove at-rest protection works.
    const SAMPLE_PROVIDER_VALUE: &str = "test-value-1234567890";

    #[test]
    fn provider_value_is_not_stored_in_plaintext() {
        let conn = open_in_memory().unwrap();
        let mut settings = load_settings(&conn).unwrap();
        settings.openai_api_key = SAMPLE_PROVIDER_VALUE.to_string();
        save_settings(&conn, &settings).unwrap();

        let raw = load_kv(&conn, "settings").unwrap().unwrap();
        assert!(
            !raw.contains(SAMPLE_PROVIDER_VALUE),
            "the raw database row must not contain the value"
        );

        // ... and it comes back intact for the current Windows user.
        let loaded = load_settings(&conn).unwrap();
        assert_eq!(loaded.openai_api_key, SAMPLE_PROVIDER_VALUE);
    }

    #[test]
    fn legacy_plaintext_values_still_load() {
        let conn = open_in_memory().unwrap();
        let mut legacy = serde_json::json!({ "wake_word": "hey nova" });
        legacy[CREDENTIAL_FIELD] = serde_json::Value::String(SAMPLE_PROVIDER_VALUE.to_string());
        save_kv(&conn, "settings", &legacy.to_string()).unwrap();

        let loaded = load_settings(&conn).unwrap();
        assert_eq!(loaded.openai_api_key, SAMPLE_PROVIDER_VALUE);
        assert_eq!(loaded.wake_word, "hey nova");
    }

    #[test]
    fn kv_rows_roundtrip() {
        let conn = open_in_memory().unwrap();
        save_kv(&conn, "appearance", "{\"theme\":\"darcula\"}").unwrap();
        assert_eq!(
            load_kv(&conn, "appearance").unwrap().as_deref(),
            Some("{\"theme\":\"darcula\"}")
        );
        #[derive(Default, serde::Deserialize, PartialEq, Debug)]
        struct Stub {
            theme: String,
        }
        let stub: Stub = load_json_or_default(&conn, "appearance").unwrap();
        assert_eq!(stub.theme, "darcula");
        // A corrupt row falls back to the default instead of failing.
        save_kv(&conn, "appearance", "not json").unwrap();
        assert_eq!(
            load_json_or_default::<Stub>(&conn, "appearance").unwrap(),
            Stub::default()
        );
        delete_kv(&conn, "appearance").unwrap();
        assert!(load_kv(&conn, "appearance").unwrap().is_none());
    }
}
