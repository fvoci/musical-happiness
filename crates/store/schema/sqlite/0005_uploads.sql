-- Uploads in parts (docs/s6-design.md 5.6): a session that a user opens for a file of a
-- board, the parts received so far, and the completion that makes the file. Nothing of
-- a session is a file until it is completed: the parts are kept apart from the files.
CREATE TABLE uploads (
    -- 256 random bits, URL-safe base64. The identifier alone grants nothing: every
    -- request is checked against the user too.
    id TEXT NOT NULL PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    board_id TEXT NOT NULL REFERENCES boards (id) ON DELETE CASCADE,
    -- The name of the file as the client gave it.
    name TEXT NOT NULL,
    -- The size of the file in bytes, as declared when the session was opened.
    size BIGINT NOT NULL,
    content_type TEXT NOT NULL,
    -- Every part but the last has this many bytes; the last has the rest.
    part_size BIGINT NOT NULL,
    create_at BIGINT NOT NULL,
    expires_at BIGINT NOT NULL,
    -- When the session was closed to new parts, before it is completed or thrown away;
    -- zero while it is open.
    closed_at BIGINT NOT NULL,
    -- The object store's identifier of the multipart upload, where the parts go to one;
    -- empty where they are kept on the local disk.
    multipart_id TEXT NOT NULL
);
CREATE INDEX uploads_user_id_idx ON uploads (user_id);
CREATE INDEX uploads_expires_at_idx ON uploads (expires_at);

CREATE TABLE upload_parts (
    upload_id TEXT NOT NULL REFERENCES uploads (id) ON DELETE CASCADE,
    -- From 1.
    number BIGINT NOT NULL,
    size BIGINT NOT NULL,
    -- The object store's identifier of the part (its ETag); empty on the local disk.
    part_id TEXT NOT NULL,
    PRIMARY KEY (upload_id, number)
);
