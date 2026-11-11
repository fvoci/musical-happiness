-- The keys of the rows that name another row without a foreign key to it, held to the
-- organization.
--
-- These rows outlive what they name, or name what is no organization's alone, so
-- 0014_organizations.sql gave them no key to it. Their own keys still named no
-- organization: a row that one organization stored under an identifier of another's
-- took that identifier from the other, whose next write of it was refused as a
-- duplicate. Each such key is unique within an organization now, as the rows it is
-- read by are.
--
-- The wall holds the owner of the tables too: without the system's reach a new key
-- would be checked against no row.
SELECT set_config('fvoci.system', 'on', true);

-- The housekeeping matches the versions of a comment to the comment's block in its own
-- organization from now on. 0014 gave a version the organization of its block, and one
-- of a comment in the trash the one organization; but a block in the trash has its
-- board's organization, which for a comment on a built-in template is the system's.
-- Such versions take the organization of their block in the trash.
UPDATE comment_versions AS v SET org_id = d.org_id
FROM deleted_blocks AS d
WHERE d.id = v.block_id AND v.org_id <> d.org_id
  AND NOT EXISTS (SELECT 1 FROM blocks AS b WHERE b.id = v.block_id);

-- The versions of a comment and the files a block shows outlive the block for as long
-- as it is in the trash, and go with it when it goes for good.
ALTER TABLE comment_versions DROP CONSTRAINT comment_versions_pkey,
    ADD PRIMARY KEY (block_id, org_id, version);
ALTER TABLE file_refs DROP CONSTRAINT file_refs_pkey,
    ADD PRIMARY KEY (block_id, org_id, name);

-- The trash holds a block or a board under the identifier it had, which is gone from
-- `blocks` or `boards` while it is here.
ALTER TABLE deleted_blocks DROP CONSTRAINT deleted_blocks_pkey, ADD PRIMARY KEY (id, org_id);
ALTER TABLE deleted_boards DROP CONSTRAINT deleted_boards_pkey, ADD PRIMARY KEY (id, org_id);

-- A user files a board under one category of each organization: a built-in template,
-- the system organization's, once in each organization the user works in.
ALTER TABLE category_boards DROP CONSTRAINT category_boards_user_id_board_id_key,
    ADD CONSTRAINT category_boards_user_id_board_id_org_id_key UNIQUE (user_id, board_id, org_id);

-- Left without a key to what they name, and why:
--
-- * deleted_blocks.board_id, deleted_boards.team_id: the board of a block in the trash
--   may be in the trash too. For a request the trash is read through the board or the
--   workspace it names, whose rows the wall keeps to their organization, and what comes
--   back from it comes back through the keys of `blocks` and `boards`. The purge, which
--   reaches every organization, matches a block to the boards of its own organization.
--   It also hands over the files listed in `deleted_boards.files` (JSON, which no key
--   holds) to be removed: that their paths are their board's own is the purge's to
--   check, not the schema's.
-- * file_refs.board_id, comment_versions.block_id: they name what may be in the trash.
--   The housekeeping, which reads every organization, matches them to the file records
--   and the blocks of their own organization only.
--
-- Checked when the row is stored instead (below): the board or workspace of an upload,
-- which may go to the trash and from there for good while the upload's row stays for
-- its clean-up; and the board of a category_boards row, which may be a built-in
-- template of the system organization. The check reads under the wall of the
-- transaction that stores the row: a board of another organization is not there to
-- be found. Neither the board nor the workspace of a row moves to another
-- organization afterwards.

-- An upload goes to a board of its organization, or an import to a workspace of it.
CREATE FUNCTION uploads_target_of_its_organization() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    -- A row of no organization is refused by its column, which takes no NULL.
    IF NEW.org_id IS NULL THEN
        RETURN NEW;
    END IF;
    IF NEW.purpose = 'import' THEN
        IF NOT EXISTS (SELECT 1 FROM teams WHERE id = NEW.board_id AND org_id = NEW.org_id) THEN
            RAISE foreign_key_violation USING
                MESSAGE = 'the workspace of an import is not one of its organization';
        END IF;
    ELSIF NOT EXISTS (SELECT 1 FROM boards WHERE id = NEW.board_id AND org_id = NEW.org_id) THEN
        RAISE foreign_key_violation USING
            MESSAGE = 'the board of an upload is not one of its organization';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER uploads_target_of_its_organization
    BEFORE INSERT OR UPDATE OF board_id, purpose, org_id ON uploads
    FOR EACH ROW EXECUTE FUNCTION uploads_target_of_its_organization();

-- A category takes a board of its organization or a built-in template.
CREATE FUNCTION category_boards_board_of_its_organization() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    -- A row of no organization is refused by its column, which takes no NULL.
    IF NEW.org_id IS NULL THEN
        RETURN NEW;
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM boards WHERE id = NEW.board_id
        AND org_id IN (NEW.org_id, '00000000-0000-7000-8000-00000000a000')
    ) THEN
        RAISE foreign_key_violation USING
            MESSAGE = 'the board of a category is neither of its organization nor a template';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER category_boards_board_of_its_organization
    BEFORE INSERT OR UPDATE OF board_id, org_id ON category_boards
    FOR EACH ROW EXECUTE FUNCTION category_boards_board_of_its_organization();
