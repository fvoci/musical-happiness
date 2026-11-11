-- What an upload is for: `file`, a file of a board, or `import`, an archive imported
-- into a team (`docs/s6-design.md` 5.6, 9). The column `board_id` names what the upload
-- goes to: the board of a file, the team of an import. Both are identifiers, and a
-- second column would leave one of them empty, which the column takes from no upload.
ALTER TABLE uploads ADD COLUMN purpose TEXT NOT NULL DEFAULT 'file';
