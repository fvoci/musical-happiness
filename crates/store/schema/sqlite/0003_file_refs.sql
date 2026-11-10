-- Which block shows which file of its board (docs/s6-design.md 5.7, the C of "본문에서
-- 빠진 이미지 파일"): an image or attachment block its file, a card the images of its
-- body. A block in the trash keeps its rows; they go when the block is gone for good.
CREATE TABLE file_refs (
    block_id TEXT NOT NULL,
    board_id TEXT NOT NULL,
    -- The stored name of the file (file_info.name).
    name TEXT NOT NULL,
    PRIMARY KEY (block_id, name)
);
CREATE INDEX file_refs_board_id_name_idx ON file_refs (board_id, name);

-- Since when no block shows the file, in milliseconds; NULL while one does. A file that
-- stays so past the grace period is removed.
ALTER TABLE file_info ADD COLUMN unreferenced_at BIGINT;
CREATE INDEX file_info_unreferenced_at_idx ON file_info (unreferenced_at);

-- What the blocks show now, live and in the trash.
INSERT OR IGNORE INTO file_refs (block_id, board_id, name)
SELECT id, board_id, COALESCE(json_extract(fields, '$.fileId'), json_extract(fields, '$.attachmentId'))
FROM blocks
WHERE type IN ('image', 'attachment')
  AND COALESCE(json_extract(fields, '$.fileId'), json_extract(fields, '$.attachmentId')) <> '';
INSERT OR IGNORE INTO file_refs (block_id, board_id, name)
SELECT b.id, b.board_id, json_extract(node.value, '$.attrs.fileId')
FROM blocks b, json_tree(b.body) node
WHERE b.body IS NOT NULL
  -- A leaf's value is no JSON text: only objects are looked into.
  AND CASE WHEN node.type = 'object' THEN
      json_extract(node.value, '$.type') = 'image'
      AND json_type(node.value, '$.attrs.fileId') = 'text'
  ELSE 0 END;
INSERT OR IGNORE INTO file_refs (block_id, board_id, name)
SELECT id, board_id, COALESCE(json_extract(block, '$.fields.fileId'), json_extract(block, '$.fields.attachmentId'))
FROM deleted_blocks
WHERE json_extract(block, '$.type') IN ('image', 'attachment')
  AND COALESCE(json_extract(block, '$.fields.fileId'), json_extract(block, '$.fields.attachmentId')) <> '';
INSERT OR IGNORE INTO file_refs (block_id, board_id, name)
SELECT d.id, d.board_id, json_extract(node.value, '$.attrs.fileId')
FROM deleted_blocks d, json_tree(d.block, '$.body') node
WHERE json_type(d.block, '$.body') = 'object'
  AND CASE WHEN node.type = 'object' THEN
      json_extract(node.value, '$.type') = 'image'
      AND json_type(node.value, '$.attrs.fileId') = 'text'
  ELSE 0 END;

-- A file no block shows is counted from now.
UPDATE file_info SET unreferenced_at = CAST(strftime('%s', 'now') AS INTEGER) * 1000
WHERE NOT EXISTS (
    SELECT 1 FROM file_refs r WHERE r.board_id = file_info.board_id AND r.name = file_info.name
);
