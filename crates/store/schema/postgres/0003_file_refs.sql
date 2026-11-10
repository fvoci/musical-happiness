-- Which block shows which file of its board (docs/s6-design.md 5.7, the C of "본문에서
-- 빠진 이미지 파일"): an image or attachment block its file, a card the images of its
-- body. A block in the trash keeps its rows; they go when the block is gone for good.
CREATE TABLE file_refs (
    block_id UUID NOT NULL,
    board_id UUID NOT NULL,
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
INSERT INTO file_refs (block_id, board_id, name)
SELECT id, board_id, COALESCE(fields->>'fileId', fields->>'attachmentId')
FROM blocks
WHERE type IN ('image', 'attachment')
  AND COALESCE(fields->>'fileId', fields->>'attachmentId') <> ''
ON CONFLICT DO NOTHING;
INSERT INTO file_refs (block_id, board_id, name)
SELECT b.id, b.board_id, node->'attrs'->>'fileId'
FROM blocks b, jsonb_path_query(b.body, 'strict $.**') node
WHERE b.body IS NOT NULL
  AND jsonb_typeof(node) = 'object'
  AND node->>'type' = 'image'
  AND jsonb_typeof(node->'attrs'->'fileId') = 'string'
ON CONFLICT DO NOTHING;
INSERT INTO file_refs (block_id, board_id, name)
SELECT id, board_id, COALESCE(block->'fields'->>'fileId', block->'fields'->>'attachmentId')
FROM deleted_blocks
WHERE block->>'type' IN ('image', 'attachment')
  AND COALESCE(block->'fields'->>'fileId', block->'fields'->>'attachmentId') <> ''
ON CONFLICT DO NOTHING;
INSERT INTO file_refs (block_id, board_id, name)
SELECT d.id, d.board_id, node->'attrs'->>'fileId'
FROM deleted_blocks d, jsonb_path_query((d.block::jsonb)->'body', 'strict $.**') node
WHERE jsonb_typeof((d.block::jsonb)->'body') = 'object'
  AND jsonb_typeof(node) = 'object'
  AND node->>'type' = 'image'
  AND jsonb_typeof(node->'attrs'->'fileId') = 'string'
ON CONFLICT DO NOTHING;

-- A file no block shows is counted from now.
UPDATE file_info SET unreferenced_at = (EXTRACT(EPOCH FROM now()) * 1000)::BIGINT
WHERE NOT EXISTS (
    SELECT 1 FROM file_refs r WHERE r.board_id = file_info.board_id AND r.name = file_info.name
);
