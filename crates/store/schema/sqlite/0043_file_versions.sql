-- The versions of a file: the bytes it had before it was written anew, kept to be read
-- and gone back to. `file_info` names the newest (`path`, `size`) and its number
-- (`version`, from 1); a file that was never written anew has no row here, and its
-- record is its only version. From its second version on a file has a row per version
-- kept, the newest among them: the server keeps the newest (setting
-- `max_file_versions`), and the object of a version that goes is doomed with its row.
--
-- The rows stay while the board of the file is in the trash, where its record is kept
-- with the board, and go when the record goes for good: so they have no key to it.
CREATE TABLE file_versions (
    -- The file (file_info.id).
    file_id TEXT NOT NULL,
    version BIGINT NOT NULL,
    -- Where the bytes of the version are in the file storage.
    path TEXT NOT NULL,
    size BIGINT NOT NULL,
    created_at BIGINT NOT NULL,
    -- Who wrote the version; NULL where nobody is known to have.
    created_by TEXT,
    -- The version whose bytes this one took, for one made by going back; else NULL.
    restored_from BIGINT,
    PRIMARY KEY (file_id, version)
);
-- Whether something holds an object is asked by its place.
CREATE INDEX file_versions_path_idx ON file_versions (path);

-- The number of the file's newest version.
ALTER TABLE file_info ADD COLUMN version BIGINT NOT NULL DEFAULT 1;
-- Who uploaded the file: who wrote its first version. NULL for a file from before
-- this was kept, and for a copy.
ALTER TABLE file_info ADD COLUMN created_by TEXT;
