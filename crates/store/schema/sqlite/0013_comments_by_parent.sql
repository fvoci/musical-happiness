-- When a card's last comment was posted is the newest create_at of its comments, read
-- for every card of a list: one probe of this index per card.
CREATE INDEX blocks_parent_type_create_at_idx ON blocks (parent_id, type, create_at);
