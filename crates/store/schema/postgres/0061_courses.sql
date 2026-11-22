-- Courses: a fourth kind of container ('course'), whose blocks are chapters ('chapter')
-- and units ('unit').

ALTER TABLE containers DROP CONSTRAINT containers_kind_check;
ALTER TABLE containers ADD CONSTRAINT containers_kind_check
    CHECK (kind IN ('board', 'wiki', 'diary', 'course'));
