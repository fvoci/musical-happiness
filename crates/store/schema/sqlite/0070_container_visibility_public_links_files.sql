-- Container visibility (`open` or `private`, replacing `containers.type` `'O'`/`'P'`),
-- `public_links` (replacing `sharing`), and `files` (replacing `file_info`).

DROP TRIGGER diaries_of_a_space;
DROP TRIGGER diaries_stay_of_a_space;
DROP TRIGGER sharing_of_no_diary;
DROP TRIGGER sharing_stays_of_no_diary;

ALTER TABLE containers ADD COLUMN visibility TEXT NOT NULL DEFAULT 'private'
    CHECK (visibility IN ('open', 'private'));
UPDATE containers
SET visibility = CASE type
    WHEN 'O' THEN 'open'
    WHEN 'P' THEN 'private'
END;
ALTER TABLE containers DROP COLUMN type;

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

ALTER TABLE sharing RENAME TO public_links;

CREATE TRIGGER public_links_of_no_diary BEFORE INSERT ON public_links
WHEN EXISTS (SELECT 1 FROM containers AS c WHERE c.id = NEW.container_id AND c.kind = 'diary')
BEGIN
    SELECT RAISE(ABORT, 'a diary has no link');
END;
CREATE TRIGGER public_links_stay_of_no_diary BEFORE UPDATE OF container_id ON public_links
WHEN EXISTS (SELECT 1 FROM containers AS c WHERE c.id = NEW.container_id AND c.kind = 'diary')
BEGIN
    SELECT RAISE(ABORT, 'a diary has no link');
END;

ALTER TABLE file_info RENAME TO files;
DROP INDEX file_info_unreferenced_at_idx;
CREATE INDEX files_unreferenced_at_idx ON files (unreferenced_at);
DROP INDEX file_info_path_idx;
CREATE INDEX files_path_idx ON files (path);

UPDATE deleted_containers
SET container = json_set(
    json_remove(container, '$.type'),
    '$.visibility',
    CASE json_extract(container, '$.type')
        WHEN 'O' THEN 'open'
        WHEN 'P' THEN 'private'
        ELSE json_extract(container, '$.type')
    END
)
WHERE json_type(container, '$.type') IS NOT NULL;
