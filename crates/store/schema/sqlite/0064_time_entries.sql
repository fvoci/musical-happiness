-- Time entries: personal time tracking records, optionally linked to a card or unit.
CREATE TABLE time_entries (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL DEFAULT '',
    user_id TEXT NOT NULL,
    workspace_id TEXT,
    container_id TEXT,
    -- No foreign key to blocks: entries survive when their card or unit is trashed or purged.
    block_id TEXT,
    target_kind TEXT CHECK (target_kind IS NULL OR target_kind IN ('card', 'unit', 'board', 'course')),
    target_title TEXT,
    kind TEXT NOT NULL CHECK (kind IN ('lecture', 'study', 'work', 'other')),
    started_at BIGINT NOT NULL,
    -- NULL while the timer is running.
    ended_at BIGINT,
    note TEXT NOT NULL DEFAULT '',
    create_at BIGINT NOT NULL,
    update_at BIGINT NOT NULL,
    CHECK (ended_at IS NULL OR ended_at >= started_at)
);

-- At most one active (running) timer per user across all organizations.
CREATE UNIQUE INDEX time_entries_one_active_per_user
    ON time_entries (user_id)
    WHERE ended_at IS NULL;

-- Listing a user's entries by time range, newest first.
CREATE INDEX time_entries_user_started_idx
    ON time_entries (user_id, started_at DESC, id DESC);

-- Scoped queries and organization-departure cleanup.
CREATE INDEX time_entries_org_user_started_idx
    ON time_entries (org_id, user_id, started_at DESC);

-- Filtered listing by target card/unit or board/course.
CREATE INDEX time_entries_user_block_started_idx
    ON time_entries (user_id, block_id, started_at DESC, id DESC)
    WHERE block_id IS NOT NULL;

CREATE INDEX time_entries_user_container_started_idx
    ON time_entries (user_id, container_id, started_at DESC, id DESC)
    WHERE container_id IS NOT NULL;
