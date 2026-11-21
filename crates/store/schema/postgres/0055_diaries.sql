-- Diaries: a third kind of container ('diary'), whose blocks are the entries of its
-- days (blocks of the type 'entry'), one a day. Only the workspace of a user's own
-- space has one, and no diary has a link.
SELECT set_config('fvoci.system', 'on', true);

-- The kind of a container may be 'diary'.
ALTER TABLE containers DROP CONSTRAINT containers_kind_check;
ALTER TABLE containers ADD CONSTRAINT containers_kind_check
    CHECK (kind IN ('board', 'wiki', 'diary'));

-- A diary is of the space of an account: the store refuses one anywhere else before
-- this does, and says why; this holds whatever writes a container. The organization a
-- row names is the one the transaction reaches.
CREATE FUNCTION diaries_are_of_a_space() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.kind = 'diary'
       AND NOT EXISTS (SELECT 1 FROM organizations AS o
                       WHERE o.id = NEW.org_id AND o.kind = 'personal') THEN
        RAISE EXCEPTION 'a diary is of the space of an account: no other workspace has one'
            USING ERRCODE = 'restrict_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER diaries_of_a_space BEFORE INSERT OR UPDATE OF org_id, workspace_id, kind
    ON containers
    FOR EACH ROW EXECUTE FUNCTION diaries_are_of_a_space();

-- A diary has no link: nobody reads it but its owner.
CREATE FUNCTION diaries_have_no_link() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF EXISTS (SELECT 1 FROM containers AS c
               WHERE c.id = NEW.container_id AND c.org_id = NEW.org_id AND c.kind = 'diary') THEN
        RAISE EXCEPTION 'a diary has no link' USING ERRCODE = 'restrict_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER sharing_of_no_diary BEFORE INSERT OR UPDATE OF org_id, container_id ON sharing
    FOR EACH ROW EXECUTE FUNCTION diaries_have_no_link();

-- The days of a diary that have an entry: a row per entry that is among the blocks,
-- put down when the entry is stored and gone with its block (to the trash too, where
-- an entry holds no day). The entry's block says its day itself, among its fields:
-- this is that day where a query finds it without reading the entries, and where a
-- diary is held to one entry a day, by the key.
--
-- Rows of an organization, behind the wall as the tables of 0014 are. The policy
-- below reads the settings and no row; the line that follows is there because the check
-- of later migrations sees the word of its subquery (crates/store/src/schema.rs).
SELECT set_config('fvoci.system', 'on', true);
CREATE TABLE diary_days (
    container_id UUID NOT NULL,
    -- The day, '2026-11-21': days compare as their texts do.
    day TEXT NOT NULL,
    -- Its month and day without the year, '11-21': what the entries of one page of the
    -- diary share. '02-29' is a day of its own.
    month_day TEXT NOT NULL,
    entry_id UUID NOT NULL,
    -- The mood of the entry, for the calendar; NULL for none.
    mood TEXT,
    org_id UUID NOT NULL DEFAULT (NULLIF(current_setting('fvoci.org', true), ''))::uuid,
    -- The organization last: the days of a diary are next to each other in the key.
    PRIMARY KEY (container_id, day, org_id),
    UNIQUE (entry_id, org_id),
    FOREIGN KEY (entry_id, org_id) REFERENCES blocks (id, org_id) ON DELETE CASCADE
);
-- The entries of a month and day in every year.
CREATE INDEX diary_days_month_day_idx ON diary_days (container_id, month_day);

ALTER TABLE diary_days ENABLE ROW LEVEL SECURITY;
ALTER TABLE diary_days FORCE ROW LEVEL SECURITY;
CREATE POLICY org_wall ON diary_days USING (
    org_id = (SELECT NULLIF(current_setting('fvoci.org', true), '')::uuid)
    OR (SELECT current_setting('fvoci.system', true)) = 'on');
