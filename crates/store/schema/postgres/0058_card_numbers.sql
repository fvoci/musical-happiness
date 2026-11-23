-- The numbers of the cards of a board, as `#12` names the twelfth card made on it. The
-- cards that are there are numbered from 1 on each board in the order they were made,
-- those of one batch, which share a time, by identifier; a template of a board's cards
-- (`isTemplate` the JSON `true`) has no number, nor has any block that is no card. A
-- card in the trash is numbered when it comes back.
--
-- Rows of organizations: the line that follows reaches all of them.
SELECT set_config('fvoci.system', 'on', true);

-- The number the next card of the board gets. It only goes up: a number is never given
-- again, also once its card is gone for good.
ALTER TABLE containers ADD COLUMN next_card_number BIGINT NOT NULL DEFAULT 1;
-- The same of a board in the trash, which it comes back with.
ALTER TABLE deleted_containers ADD COLUMN next_card_number BIGINT NOT NULL DEFAULT 1;
-- The card's number on its board; NULL for every other block.
ALTER TABLE blocks ADD COLUMN number BIGINT;

UPDATE blocks SET number = numbered.number
FROM (SELECT id, ROW_NUMBER() OVER (PARTITION BY container_id ORDER BY create_at, id) AS number
      FROM blocks
      WHERE type = 'card' AND (fields->'isTemplate')::text IS DISTINCT FROM 'true') AS numbered
WHERE numbered.id = blocks.id;

UPDATE containers SET next_card_number = 1 + COALESCE(
    (SELECT MAX(b.number) FROM blocks AS b WHERE b.container_id = containers.id), 0);

-- No two cards of a board have one number; a card is found by its number here.
CREATE UNIQUE INDEX blocks_container_id_number_idx ON blocks (container_id, number);
