-- Nova schema migration 0002
-- Adds the task fields the V3 designs surface (due date, priority, category,
-- origin) and the richer activity-log columns the Activity screen needs.
--
-- This migration is additive only: existing rows keep their data and receive
-- sensible defaults, so an existing user database is never reset.

-- ── tasks ───────────────────────────────────────────────────────────────────
-- priority: 'low' | 'medium' | 'high' (validated in Rust, defaulted here).
ALTER TABLE todos ADD COLUMN priority TEXT NOT NULL DEFAULT 'medium';
-- category: free-form grouping shown as a chip in the Tasks list ('' = none).
ALTER TABLE todos ADD COLUMN category TEXT NOT NULL DEFAULT '';
-- due_at: UTC RFC 3339 timestamp (NULL = no due date). UTC keeps lexicographic
-- comparison correct for the "overdue" filter.
ALTER TABLE todos ADD COLUMN due_at TEXT;
-- source: 'voice' | 'manual' | 'onboarding' — voice-created tasks carry a mark.
ALTER TABLE todos ADD COLUMN source TEXT NOT NULL DEFAULT 'manual';

CREATE INDEX IF NOT EXISTS idx_todos_due ON todos (due_at);
CREATE INDEX IF NOT EXISTS idx_todos_priority ON todos (priority);

-- ── activity log ────────────────────────────────────────────────────────────
-- status: 'success' | 'failed' | 'cancelled' | 'awaiting_confirmation'
--         | 'unsupported' | 'timeout'
ALTER TABLE activity_log ADD COLUMN status TEXT NOT NULL DEFAULT 'success';
-- heard: the exact transcript Nova acted on (kept separate from `transcript`,
-- which may be rewritten/normalized later).
ALTER TABLE activity_log ADD COLUMN heard TEXT NOT NULL DEFAULT '';
-- understood: short human description of the interpreted intent + arguments.
ALTER TABLE activity_log ADD COLUMN understood TEXT;
-- action: what Nova actually did (or refused to do).
ALTER TABLE activity_log ADD COLUMN action TEXT;
-- retryable: 1 when the entry can be re-run from the Activity screen.
ALTER TABLE activity_log ADD COLUMN retryable INTEGER NOT NULL DEFAULT 0;
-- intent_label: human label of the intent (e.g. 'Add task').
ALTER TABLE activity_log ADD COLUMN intent_label TEXT;

CREATE INDEX IF NOT EXISTS idx_activity_status ON activity_log (status);

-- Backfill: rows written before this migration have no `heard`/`intent_label`
-- values (the columns were just created). The transcript they acted on is the
-- best available value for `heard`, and the intent is the best label.
UPDATE activity_log SET heard = transcript WHERE heard = '' OR heard IS NULL;
UPDATE activity_log SET intent_label = intent WHERE intent_label IS NULL AND intent IS NOT NULL;
