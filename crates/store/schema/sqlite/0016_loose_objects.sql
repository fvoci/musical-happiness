-- Objects of the file storage that may have no record. A file's object and its record
-- are written apart, the object store and the database being two: what is listed here
-- is an object whose record is not written yet ('writing', listed before the object is
-- written), or one whose record went and that is to be removed ('doomed', listed in the
-- transaction that removes the record). The server's housekeeping removes the doomed
-- ones, trying again after a failure, and dooms a mark of writing that is older than
-- any write takes. Recording a file takes its mark of writing away in the transaction
-- that writes the record.
CREATE TABLE loose_objects (
    -- The place in the file storage, as a file's record names it.
    path TEXT NOT NULL PRIMARY KEY,
    state TEXT NOT NULL CHECK (state IN ('writing', 'doomed')),
    create_at BIGINT NOT NULL,
    -- When the housekeeping next looks at it: for a mark of writing, when it is old
    -- enough to be doomed; for a doomed object, when its removal is tried (again).
    retry_at BIGINT NOT NULL,
    -- How many removals failed, when the last was tried, and the kind of its error
    -- (empty before the first failure).
    attempts BIGINT NOT NULL DEFAULT 0,
    last_attempt_at BIGINT NOT NULL DEFAULT 0,
    last_error TEXT NOT NULL DEFAULT ''
);
CREATE INDEX loose_objects_state_retry_at_idx ON loose_objects (state, retry_at);

-- Whether something holds an object is asked by its place, of the records and of the
-- upload sessions.
CREATE INDEX file_info_path_idx ON file_info (path);
CREATE INDEX uploads_path_idx ON uploads (path);
