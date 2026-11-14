-- The one-use guest link made for one recipient of a mailed guest link: the link it was
-- made like (its parent), and the address it was mailed to. Revoking the parent revokes
-- the children nobody used yet; a link deleted takes its children along.
ALTER TABLE invitations ADD COLUMN parent_id TEXT REFERENCES invitations (id) ON DELETE CASCADE;
ALTER TABLE invitations ADD COLUMN recipient TEXT;
CREATE INDEX invitations_parent_id_idx ON invitations (parent_id) WHERE parent_id IS NOT NULL;
