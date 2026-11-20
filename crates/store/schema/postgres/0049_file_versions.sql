-- The versions of a file: the bytes it had before it was written anew, kept to be read
-- and gone back to. `file_info` names the newest (`path`, `size`) and its number
-- (`version`, from 1); a file that was never written anew has no row here, and its
-- record is its only version. From its second version on a file has a row per version
-- kept, the newest among them: the server keeps the newest (setting
-- `max_file_versions`), and the object of a version that goes is doomed with its row.
--
-- The rows stay while the board of the file is in the trash, where its record is kept
-- with the board, and go when the record goes for good: so they have no key to it.
--
-- Rows of an organization, behind the wall as the tables of 0014 are. The policy
-- below reads the settings and no row; the line that follows is there because the check
-- of later migrations sees the word of its subquery (crates/store/src/schema.rs).
SELECT set_config('fvoci.system', 'on', true);
CREATE TABLE file_versions (
    -- The file (file_info.id).
    file_id UUID NOT NULL,
    version BIGINT NOT NULL,
    -- Where the bytes of the version are in the file storage.
    path TEXT NOT NULL,
    size BIGINT NOT NULL,
    created_at BIGINT NOT NULL,
    -- Who wrote the version; NULL where nobody is known to have.
    created_by UUID,
    -- The version whose bytes this one took, for one made by going back; else NULL.
    restored_from BIGINT,
    org_id UUID NOT NULL DEFAULT (NULLIF(current_setting('fvoci.org', true), ''))::uuid,
    PRIMARY KEY (file_id, org_id, version)
);
-- Whether something holds an object is asked by its place.
CREATE INDEX file_versions_path_idx ON file_versions (path);

ALTER TABLE file_versions ENABLE ROW LEVEL SECURITY;
ALTER TABLE file_versions FORCE ROW LEVEL SECURITY;
CREATE POLICY org_wall ON file_versions USING (
    org_id = (SELECT NULLIF(current_setting('fvoci.org', true), '')::uuid)
    OR (SELECT current_setting('fvoci.system', true)) = 'on');

-- The number of the file's newest version.
ALTER TABLE file_info ADD COLUMN version BIGINT NOT NULL DEFAULT 1;
-- Who uploaded the file: who wrote its first version. NULL for a file from before
-- this was kept, and for a copy.
ALTER TABLE file_info ADD COLUMN created_by UUID;
