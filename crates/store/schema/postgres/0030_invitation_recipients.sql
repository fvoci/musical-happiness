-- The one-use guest link made for one recipient of a mailed guest link: the link it was
-- made like (its parent), and the address it was mailed to. Revoking the parent revokes
-- the children nobody used yet; a link deleted takes its children along. A child is
-- its parent's organization's, as the key says.
SELECT set_config('fvoci.system', 'on', true);

ALTER TABLE invitations ADD COLUMN parent_id UUID;
ALTER TABLE invitations ADD COLUMN recipient TEXT;
ALTER TABLE invitations ADD CONSTRAINT invitations_parent_fkey
    FOREIGN KEY (parent_id, org_id) REFERENCES invitations (id, org_id) ON DELETE CASCADE;
CREATE INDEX invitations_parent_id_idx ON invitations (parent_id) WHERE parent_id IS NOT NULL;
