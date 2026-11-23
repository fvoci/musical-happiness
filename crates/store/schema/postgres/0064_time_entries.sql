-- Time entries: personal time tracking records, optionally linked to a card or unit.
--
-- Rows of an organization, behind the wall as the tables of 0014 are. The policy
-- below reads the settings and no row; the line that follows is there because the check
-- of later migrations sees the word of its subquery (crates/store/src/schema.rs).
SELECT set_config('fvoci.system', 'on', true);
CREATE TABLE time_entries (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL DEFAULT (NULLIF(current_setting('fvoci.org', true), ''))::uuid,
    user_id UUID NOT NULL,
    workspace_id UUID,
    container_id UUID,
    -- No foreign key to blocks: entries survive when their card or unit is trashed or purged.
    block_id UUID,
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

ALTER TABLE time_entries ENABLE ROW LEVEL SECURITY;
ALTER TABLE time_entries FORCE ROW LEVEL SECURITY;
CREATE POLICY org_wall ON time_entries USING (
    org_id = (SELECT NULLIF(current_setting('fvoci.org', true), '')::uuid)
    OR (SELECT current_setting('fvoci.system', true)) = 'on');
