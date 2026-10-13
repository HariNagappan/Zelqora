//! Short-lived conversation context (spec §14): a bounded window of recent
//! exchanges plus the last referenced todo / application, used for pronoun
//! resolution ("complete it", "close that") and to build a compact prompt for
//! the cloud model. Never persisted; never sent to the cloud unbounded.

use std::collections::VecDeque;

use serde::Serialize;

/// How many exchanges we keep in the context window.
const MAX_EXCHANGES: usize = 8;
/// How many exchanges are compressed into the LLM prompt.
const PROMPT_EXCHANGES: usize = 4;

#[derive(Debug, Clone, Serialize)]
pub struct Exchange {
    pub transcript: String,
    pub intent: Option<String>,
    pub success: bool,
    /// A short human-readable description of what happened.
    pub summary: String,
    pub response: String,
}

#[derive(Debug, Clone, Default)]
pub struct ConversationContext {
    exchanges: VecDeque<Exchange>,
    /// The most recently referenced todo id (set from successful todo results).
    pub last_todo_id: Option<i64>,
    /// The most recently launched application.
    pub last_app: Option<String>,
    /// The intent of the last interpreted command (used for activity logging
    /// and for continuing a clarification question).
    pub last_intent: Option<String>,
}

impl ConversationContext {
    pub fn push(&mut self, exchange: Exchange) {
        if self.exchanges.len() >= MAX_EXCHANGES {
            self.exchanges.pop_front();
        }
        self.exchanges.push_back(exchange);
    }

    pub fn last_todo_id(&self) -> Option<i64> {
        self.last_todo_id
    }

    /// A compact summary of the recent conversation for the LLM system prompt.
    pub fn compact_summary(&self) -> String {
        if self.exchanges.is_empty() {
            return String::new();
        }
        let mut lines = Vec::new();
        for exchange in self.exchanges.iter().rev().take(PROMPT_EXCHANGES).rev() {
            let mut line = format!("- User: \"{}\"", truncate(&exchange.transcript, 120));
            if let Some(intent) = &exchange.intent {
                line.push_str(&format!(" -> {} ({})", intent, if exchange.success { "ok" } else { "failed" }));
            } else {
                line.push_str(" -> no command");
            }
            line.push_str(&format!(" | Nova: \"{}\"", truncate(&exchange.response, 140)));
            lines.push(line);
        }
        let mut summary = lines.join("\n");
        if let Some(id) = self.last_todo_id {
            summary.push_str(&format!("\n- Last edited todo id: {id}"));
        }
        if let Some(app) = &self.last_app {
            summary.push_str(&format!("\n- Last opened app: {app}"));
        }
        if summary.len() > 1200 {
            summary.truncate(1200);
            summary.push('…');
        }
        summary
    }
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        let cut: String = text.chars().take(max).collect();
        format!("{cut}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_window_is_bounded() {
        let mut ctx = ConversationContext::default();
        for i in 0..20 {
            ctx.push(Exchange {
                transcript: format!("command {i}"),
                intent: Some("LIST_TODOS".into()),
                success: true,
                summary: "listed".into(),
                response: "done".into(),
            });
        }
        assert_eq!(ctx.exchanges.len(), MAX_EXCHANGES);
        let summary = ctx.compact_summary();
        assert!(summary.contains("command 19"));
        assert!(!summary.contains("command 0"));
    }

    #[test]
    fn summary_mentions_last_todo() {
        let mut ctx = ConversationContext::default();
        ctx.last_todo_id = Some(42);
        ctx.push(Exchange {
            transcript: "add milk".into(),
            intent: Some("ADD_TODO".into()),
            success: true,
            summary: "added".into(),
            response: "Added milk.".into(),
        });
        assert!(ctx.compact_summary().contains("42"));
    }
}
