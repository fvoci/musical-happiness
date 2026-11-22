-- Diaries: a third kind of container ('diary'), whose blocks are the entries of its
-- days (blocks of the type 'entry'), one a day. Only the workspace of a user's own
-- space has one, and no diary has a link.

-- The kind of a container may be 'diary'. SQLite changes no check in place, and the
-- check is the column's own: the column is made anew with the wider check, filled from
-- the old one, and takes its name. The table is not made again (its rows are written
-- twice where they stand): what refers to a container keeps referring to it, and no
-- key does what it does when a container goes. No index and no trigger names the
-- column.
ALTER TABLE containers ADD COLUMN kind_wider TEXT NOT NULL DEFAULT 'board'
    CHECK (kind_wider IN ('board', 'wiki', 'diary'));
UPDATE containers SET kind_wider = kind;
ALTER TABLE containers DROP COLUMN kind;
ALTER TABLE containers RENAME COLUMN kind_wider TO kind;

-- A diary is of the space of an account: the store refuses one anywhere else before
-- this does, and says why; this holds whatever writes a container.
CREATE TRIGGER diaries_of_a_space BEFORE INSERT ON containers
WHEN NEW.kind = 'diary'
     AND NOT EXISTS (SELECT 1 FROM workspaces AS w JOIN organizations AS o ON o.id = w.org_id
                     WHERE w.id = NEW.workspace_id AND o.kind = 'personal')
BEGIN
    SELECT RAISE(ABORT, 'a diary is of the space of an account: no other workspace has one');
END;
CREATE TRIGGER diaries_stay_of_a_space BEFORE UPDATE OF workspace_id, kind ON containers
WHEN NEW.kind = 'diary'
     AND NOT EXISTS (SELECT 1 FROM workspaces AS w JOIN organizations AS o ON o.id = w.org_id
                     WHERE w.id = NEW.workspace_id AND o.kind = 'personal')
BEGIN
    SELECT RAISE(ABORT, 'a diary is of the space of an account: no other workspace has one');
END;

-- A diary has no link: nobody reads it but its owner.
CREATE TRIGGER sharing_of_no_diary BEFORE INSERT ON sharing
WHEN EXISTS (SELECT 1 FROM containers AS c WHERE c.id = NEW.container_id AND c.kind = 'diary')
BEGIN
    SELECT RAISE(ABORT, 'a diary has no link');
END;
CREATE TRIGGER sharing_stays_of_no_diary BEFORE UPDATE OF container_id ON sharing
WHEN EXISTS (SELECT 1 FROM containers AS c WHERE c.id = NEW.container_id AND c.kind = 'diary')
BEGIN
    SELECT RAISE(ABORT, 'a diary has no link');
END;

-- The days of a diary that have an entry: a row per entry that is among the blocks,
-- put down when the entry is stored and gone with its block (to the trash too, where
-- an entry holds no day). The entry's block says its day itself, among its fields:
-- this is that day where a query finds it without reading the entries, and where a
-- diary is held to one entry a day, by the key.
CREATE TABLE diary_days (
    container_id TEXT NOT NULL,
    -- The day, '2026-11-21': days compare as their texts do.
    day TEXT NOT NULL,
    -- Its month and day without the year, '11-21': what the entries of one page of the
    -- diary share. '02-29' is a day of its own.
    month_day TEXT NOT NULL,
    entry_id TEXT NOT NULL UNIQUE REFERENCES blocks (id) ON DELETE CASCADE,
    -- The mood of the entry, for the calendar; NULL for none.
    mood TEXT,
    PRIMARY KEY (container_id, day)
);
-- The entries of a month and day in every year.
CREATE INDEX diary_days_month_day_idx ON diary_days (container_id, month_day);
