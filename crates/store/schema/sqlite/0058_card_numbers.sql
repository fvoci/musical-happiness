-- The numbers of the cards of a board: the same as schema/postgres/0058_card_numbers.sql,
-- which says what each is.
ALTER TABLE containers ADD COLUMN next_card_number BIGINT NOT NULL DEFAULT 1;
ALTER TABLE deleted_containers ADD COLUMN next_card_number BIGINT NOT NULL DEFAULT 1;
ALTER TABLE blocks ADD COLUMN number BIGINT;

UPDATE blocks SET number = numbered.number
FROM (SELECT id, ROW_NUMBER() OVER (PARTITION BY container_id ORDER BY create_at, id) AS number
      FROM blocks
      WHERE type = 'card' AND json_type(fields, '$.isTemplate') IS NOT 'true') AS numbered
WHERE numbered.id = blocks.id;

UPDATE containers SET next_card_number = 1 + COALESCE(
    (SELECT MAX(b.number) FROM blocks AS b WHERE b.container_id = containers.id), 0);

CREATE UNIQUE INDEX blocks_container_id_number_idx ON blocks (container_id, number);
