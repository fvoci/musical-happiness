//! Importing boards from the archive format of version 3 (`super::export`).
//!
//! Ports `server/app/import.go`. An import generates fresh identifiers for everything,
//! fixes up references, stores the files, points the blocks at them and adds the
//! importing user as an admin. The formats of the original (the line-based version 1
//! and the ZIP of version 2) are refused (`docs/s6-design.md` 3.10).

use std::collections::{HashMap, HashSet};
use std::io::{Cursor, Read as _};

use bytes::Bytes;
use fvoci_model::{
    ArchiveHeader, ArchiveLine, Block, BlockType, Board, BoardMember, BoardsAndBlocks,
    BoardsAndBlocksError, JsonMap, renew_boards_and_blocks_ids,
};
use serde_json::Value;

use super::App;
use super::export::ARCHIVE_VERSION;
use crate::auth::now_millis;
use crate::error::AppError;

/// The largest a single archive entry may decompress to. The original bounds only its
/// `board.jsonl`; this bounds every entry, so that a small upload cannot unfold into an
/// arbitrarily large file. It is a bound per entry, not for the archive as a whole.
const MAX_ENTRY_BYTES: u64 = 256 * 1024 * 1024;

/// The largest `board.jsonl` that is read: the original's
/// `importMaxFileSize` (`server/app/import.go:24`).
const MAX_JSONL_BYTES: usize = 70 * 1024 * 1024;

/// How an archive is imported.
#[derive(Debug, Default)]
pub(crate) struct ImportOptions {
    /// The team the boards are imported into.
    pub(crate) team_id: String,
    /// The user who imports, recorded as the modifier and added as an admin.
    pub(crate) modified_by: String,
}

impl App {
    /// Imports an archive into a team on behalf of a user. The entry point for the
    /// import route.
    ///
    /// # Errors
    ///
    /// Returns the errors of [`App::import_archive`].
    pub(crate) async fn import_team_archive(
        &self,
        archive: Bytes,
        team_id: &str,
        modified_by: &str,
    ) -> Result<(), AppError> {
        let options = ImportOptions {
            team_id: team_id.to_owned(),
            modified_by: modified_by.to_owned(),
        };
        self.import_archive(archive, &options).await
    }

    /// Imports an archive of zero or more boards with their blocks and files.
    ///
    /// # Errors
    ///
    /// Returns a bad request if the buffer is not a ZIP, has no `version.json` or one of
    /// another version, or holds what does not fit together, and the store's error if a
    /// board cannot be stored.
    pub(crate) async fn import_archive(
        &self,
        archive: Bytes,
        options: &ImportOptions,
    ) -> Result<(), AppError> {
        if archive.starts_with(br#"{"version":1"#) {
            return Err(AppError::bad_request(format_args!(
                "an archive of version 1 is not read; this server reads version {ARCHIVE_VERSION}"
            )));
        }
        let zip = zip::ZipArchive::new(Cursor::new(archive))
            .map_err(|error| AppError::bad_request(format_args!("{error}")))?;
        // The original's streaming reader rejects a ZIP with no entries as invalid.
        if zip.is_empty() {
            return Err(AppError::bad_request("zip: not a valid zip file"));
        }

        // The version first: nothing of an archive of another version is stored.
        let version = (0..zip.len())
            .map(|index| entry_name(&zip, index).map(|name| (index, name)))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .find(|(_, (dir, filename))| dir == "." && filename == "version.json")
            .map(|(index, _)| index)
            .ok_or_else(|| {
                AppError::bad_request(format_args!(
                    "the archive has no version.json; this server reads version {ARCHIVE_VERSION}"
                ))
            })?;
        check_version(&read_entry(&zip, version).await?)?;
        // Every entry is read through once before anything is stored: an entry that is
        // broken or too large refuses the archive while nothing of it is stored yet.
        for index in 0..zip.len() {
            check_entry(&zip, index).await?;
        }

        // First pass: the boards; a file entry needs its board already known.
        let mut board_map: HashMap<String, Board> = HashMap::new();
        for index in 0..zip.len() {
            let (dir, filename) = entry_name(&zip, index)?;
            // Files are read in the second pass only.
            if filename == "board.jsonl" {
                let bytes = read_entry(&zip, index).await?;
                let board = self.import_board_jsonl(bytes, options).await?;
                board_map.insert(dir, board);
            }
        }

        // Second pass: the files, placed under their board.
        let mut file_map: HashMap<(String, String), String> = HashMap::new();
        for index in 0..zip.len() {
            let (dir, filename) = entry_name(&zip, index)?;
            // An entry without a file name is a directory of a re-packed archive.
            if filename.is_empty() || filename == "version.json" || filename == "board.jsonl" {
                continue;
            }
            let Some(board) = board_map.get(&dir) else {
                tracing::warn!(dir, filename, "skipping orphan image in archive");
                continue;
            };
            let bytes = read_entry(&zip, index).await?;
            let new_name = self
                .save_file(&mut bytes.as_ref(), &board.id, &filename)
                .await
                .map_err(|error| {
                    error.context(format_args!(
                        "cannot import file {filename} for board {dir}"
                    ))
                })?;
            file_map.insert((board.id.clone(), filename), new_name);
        }

        self.fix_images_attachments(&board_map, &file_map, &options.modified_by)
            .await;
        tracing::debug!(boards_imported = board_map.len(), "import archive - done");
        Ok(())
    }

    /// Points every block of each imported board that shows a file at the newly stored
    /// one: the `fileId` or `attachmentId` of an image or attachment block, and the
    /// images of a body. A name the archive brought no file for becomes the empty name
    /// in a block's fields and stays as it is in a body. Failures are logged, as in the
    /// original.
    async fn fix_images_attachments(
        &self,
        board_map: &HashMap<String, Board>,
        file_map: &HashMap<(String, String), String>,
        user_id: &str,
    ) {
        for board in board_map.values() {
            let blocks = match self.store().get_blocks(&board_blocks(&board.id)).await {
                Ok(blocks) => blocks,
                Err(error) => {
                    tracing::error!(%error, board_id = board.id, "cannot retrieve imported blocks for board");
                    continue;
                }
            };

            let mut block_ids = Vec::new();
            let mut block_patches = Vec::new();
            for block in blocks {
                let patch =
                    if let Some(mut body) = block.body.filter(|body| !body.file_ids().is_empty()) {
                        body.rename_files(|old| {
                            file_map.get(&(board.id.clone(), old.to_owned())).cloned()
                        });
                        fvoci_model::BlockPatch {
                            body: fvoci_model::BodyChange::Set(body),
                            ..fvoci_model::BlockPatch::default()
                        }
                    } else if block.block_type == BlockType::Image
                        || block.block_type == BlockType::Attachment
                    {
                        let mut updated_fields = JsonMap::new();
                        for key in ["fileId", "attachmentId"] {
                            if let Some(old) = block.fields.get(key).and_then(Value::as_str) {
                                let new_name = file_map
                                    .get(&(board.id.clone(), old.to_owned()))
                                    .cloned()
                                    .unwrap_or_default();
                                updated_fields.insert(key.to_owned(), Value::String(new_name));
                            }
                        }
                        if updated_fields.is_empty() {
                            continue;
                        }
                        fvoci_model::BlockPatch {
                            updated_fields,
                            ..fvoci_model::BlockPatch::default()
                        }
                    } else {
                        continue;
                    };
                block_ids.push(block.id);
                block_patches.push(patch);
            }
            if block_ids.is_empty() {
                continue;
            }

            let batch = fvoci_model::BlockPatchBatch {
                block_ids,
                block_patches,
            };
            if let Err(error) = self
                .patch_blocks(
                    &board.team_id,
                    &batch,
                    user_id,
                    super::ChangeNotification::Send,
                )
                .await
            {
                tracing::error!(%error, board_id = board.id, "the imported blocks could not be pointed at their files");
            }
        }
    }

    /// Imports one board's `board.jsonl` and returns the stored board.
    ///
    /// # Errors
    ///
    /// Returns a bad request if a line is malformed or of an unknown type, or the store's
    /// error if the board cannot be stored.
    async fn import_board_jsonl(
        &self,
        jsonl: Bytes,
        options: &ImportOptions,
    ) -> Result<Board, AppError> {
        if jsonl.len() > MAX_JSONL_BYTES {
            return Err(AppError::bad_request(
                "error parsing archive line 1: size limit exceeded",
            ));
        }
        // Parsing tens of megabytes of JSON would hold up a worker of the runtime.
        let (team_id, modified_by) = (options.team_id.clone(), options.modified_by.clone());
        let (mut boards_and_blocks, board_members) = tokio::task::spawn_blocking(move || {
            parse_archive_lines(&jsonl, &team_id, &modified_by)
        })
        .await
        .map_err(|error| AppError::internal(format_args!("archive task failed: {error}")))??;

        // Keep only members who are users of this server.
        let mut kept_members = Vec::with_capacity(board_members.len());
        for member in board_members {
            if self.store().get_user_by_id(&member.user_id).await.is_ok() {
                kept_members.push(member);
            }
        }

        clear_values_of_another_kind(&mut boards_and_blocks);
        let orphans = drop_orphans(&mut boards_and_blocks);
        if orphans > 0 {
            tracing::warn!(
                orphans,
                "skipped blocks of the archive whose parent is not in the archive"
            );
        }

        let boards_and_blocks = match boards_and_blocks.validate() {
            // A board without blocks is a board all the same, and what exporting one
            // gives. The original refuses such an archive.
            Ok(()) | Err(BoardsAndBlocksError::NoBlocks) => {
                renew_boards_and_blocks_ids(boards_and_blocks)
            }
            Err(error) => {
                return Err(AppError::bad_request(format_args!(
                    "the archive's boards and blocks do not fit together: {error}"
                )));
            }
        };
        let created = self
            .create_boards_and_blocks(boards_and_blocks, &options.modified_by)
            .await
            .map_err(|error| error.context("error inserting archive blocks"))?;

        for board in &created.boards {
            let admin = BoardMember {
                board_id: board.id.clone(),
                user_id: options.modified_by.clone(),
                scheme_admin: true,
                ..BoardMember::default()
            };
            self.add_member_to_board(&admin).await?;
            for member in &kept_members {
                let member = BoardMember {
                    board_id: board.id.clone(),
                    ..member.clone()
                };
                self.add_member_to_board(&member).await?;
            }
        }

        created
            .boards
            .into_iter()
            .next()
            .ok_or_else(|| AppError::bad_request("missing board in archive"))
    }
}

/// Removes the blocks that nothing in the archive leads to, and returns how many went:
/// those whose parent is neither a block nor a board of the archive, the blocks below
/// them, and blocks that are each other's parents without any of them being on a board.
///
/// Such a block belongs to nothing that is imported: no card lists it, and its parent
/// would be a reference to something outside the import.
///
/// One walk down from the blocks that are directly on a board; the time it takes grows
/// with the number of blocks and not with how deep they are nested.
fn drop_orphans(boards_and_blocks: &mut BoardsAndBlocks) -> usize {
    let BoardsAndBlocks { boards, blocks } = boards_and_blocks;
    let board_ids: HashSet<&str> = boards.iter().map(|board| board.id.as_str()).collect();
    let mut below: HashMap<&str, Vec<usize>> = HashMap::new();
    let mut reached: Vec<usize> = Vec::new();
    for (index, block) in blocks.iter().enumerate() {
        if block.parent_id.is_empty() || board_ids.contains(block.parent_id.as_str()) {
            reached.push(index);
        } else {
            below
                .entry(block.parent_id.as_str())
                .or_default()
                .push(index);
        }
    }
    let mut keep = vec![false; blocks.len()];
    while let Some(index) = reached.pop() {
        if !std::mem::replace(&mut keep[index], true) {
            reached.extend(below.get(blocks[index].id.as_str()).into_iter().flatten());
        }
    }

    let before = blocks.len();
    let mut kept = keep.into_iter();
    blocks.retain(|_| kept.next().unwrap_or(false));
    before - blocks.len()
}

/// Parses a board's `board.jsonl` into its boards, blocks and members, with fresh
/// modification metadata but the identifiers from the archive.
///
/// # Errors
///
/// Returns a bad request if a line is malformed or of an unknown type.
fn parse_archive_lines(
    jsonl: &[u8],
    team_id: &str,
    modified_by: &str,
) -> Result<(BoardsAndBlocks, Vec<BoardMember>), AppError> {
    let modifier_user = modified_by;
    let now = now_millis();
    let mut boards_and_blocks = BoardsAndBlocks::default();
    let mut board_members: Vec<BoardMember> = Vec::new();
    let mut board_id = String::new();

    for (number, raw) in jsonl.split(|&byte| byte == b'\n').enumerate() {
        let line_number = number + 1;
        let line = trim(raw);
        if line.is_empty() {
            continue;
        }
        let archive_line: ArchiveLine = serde_json::from_slice(line).map_err(|error| {
            AppError::bad_request(format_args!(
                "error parsing archive line {line_number}: {error}"
            ))
        })?;
        match archive_line.line_type.as_str() {
            "board" => {
                let mut board: Board =
                    serde_json::from_value(archive_line.data).map_err(|error| {
                        AppError::bad_request(format_args!(
                            "invalid board in archive line {line_number}: {error}"
                        ))
                    })?;
                modifier_user.clone_into(&mut board.modified_by);
                board.update_at = now;
                team_id.clone_into(&mut board.team_id);
                board_id.clone_from(&board.id);
                boards_and_blocks.boards.push(board);
            }
            "block" => {
                let mut block: Block =
                    serde_json::from_value(archive_line.data).map_err(|error| {
                        AppError::bad_request(format_args!(
                            "invalid block in archive line {line_number}: {error}"
                        ))
                    })?;
                modifier_user.clone_into(&mut block.modified_by);
                block.update_at = now;
                block.board_id.clone_from(&board_id);
                boards_and_blocks.blocks.push(block);
            }
            "boardMember" => {
                let member: BoardMember =
                    serde_json::from_value(archive_line.data).map_err(|error| {
                        AppError::bad_request(format_args!(
                            "invalid board Member in archive line {line_number}: {error}"
                        ))
                    })?;
                board_members.push(member);
            }
            other => {
                return Err(AppError::bad_request(format_args!(
                    "unsupported archive line type; got {other}, line {line_number}"
                )));
            }
        }
    }
    Ok((boards_and_blocks, board_members))
}

/// A filter for all of a board's blocks.
fn board_blocks(board_id: &str) -> fvoci_store::BlockFilter {
    fvoci_store::BlockFilter::board(board_id)
}

/// The archive as it is held while it is imported. Cloning it copies no content.
type Archive = zip::ZipArchive<Cursor<Bytes>>;

/// Returns the cleaned directory and the base file name of an archive entry, without
/// reading its content.
fn entry_name(zip: &Archive, index: usize) -> Result<(String, String), AppError> {
    let name = zip
        .name_for_index(index)
        .ok_or_else(|| AppError::bad_request("invalid archive entry"))?
        .map_err(|error| {
            AppError::bad_request(format_args!("invalid archive entry name: {error}"))
        })?;
    Ok(split_entry_name(&name))
}

/// Reads the content of an archive entry, bounding how much it may decompress to.
/// Decompression is done off the runtime's workers, which it would hold up.
async fn read_entry(zip: &Archive, index: usize) -> Result<Bytes, AppError> {
    let mut zip = zip.clone();
    tokio::task::spawn_blocking(move || {
        let mut bytes = Vec::new();
        decompress(&mut zip, index, &mut bytes)?;
        Ok(Bytes::from(bytes))
    })
    .await
    .map_err(|error| AppError::internal(format_args!("archive task failed: {error}")))?
}

/// Reads an archive entry through without keeping it, as [`read_entry`] would read it:
/// its checksum and its bound are checked.
async fn check_entry(zip: &Archive, index: usize) -> Result<(), AppError> {
    let mut zip = zip.clone();
    tokio::task::spawn_blocking(move || decompress(&mut zip, index, &mut std::io::sink()))
        .await
        .map_err(|error| AppError::internal(format_args!("archive task failed: {error}")))?
}

/// Decompresses an archive entry into `out`, at most [`MAX_ENTRY_BYTES`] of it. The
/// reader checks the entry's checksum when it reaches the end.
fn decompress(
    zip: &mut Archive,
    index: usize,
    out: &mut impl std::io::Write,
) -> Result<(), AppError> {
    let mut entry = zip
        .by_index(index)
        .map_err(|error| AppError::bad_request(format_args!("{error}")))?;
    let read =
        std::io::copy(&mut entry.by_ref().take(MAX_ENTRY_BYTES + 1), out).map_err(|error| {
            AppError::bad_request(format_args!("cannot read archive entry: {error}"))
        })?;
    if read > MAX_ENTRY_BYTES {
        return Err(AppError::bad_request(
            "archive entry exceeds the size limit",
        ));
    }
    Ok(())
}

/// Splits an archive entry name into its cleaned directory and its base file name, as
/// the original does with `filepath.Split` and `path.Clean`. The directory is only ever
/// a map key, and the file name never has a path separator, so a `..` in the name cannot
/// escape the storage root.
fn split_entry_name(name: &str) -> (String, String) {
    match name.rsplit_once('/') {
        Some((dir, filename)) => (clean_dir(dir), filename.to_owned()),
        None => (".".to_owned(), name.to_owned()),
    }
}

/// Normalises a directory path the way Go's `path.Clean` does for the names this import
/// meets: it resolves `.` and `..` segments. The result is used only as a map key.
fn clean_dir(dir: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for segment in dir.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                if matches!(parts.last(), Some(&last) if last != "..") {
                    parts.pop();
                } else {
                    parts.push("..");
                }
            }
            other => parts.push(other),
        }
    }
    if parts.is_empty() {
        ".".to_owned()
    } else {
        parts.join("/")
    }
}

/// Validates an archive's `version.json`.
fn check_version(bytes: &[u8]) -> Result<(), AppError> {
    let header: ArchiveHeader = serde_json::from_slice(bytes).map_err(|error| {
        AppError::bad_request(format_args!("cannot parse version.json: {error}"))
    })?;
    if header.version == ARCHIVE_VERSION {
        Ok(())
    } else {
        Err(AppError::bad_request(format_args!(
            "unsupported archive version; got {}, want {ARCHIVE_VERSION}",
            header.version
        )))
    }
}

/// Trims ASCII whitespace from both ends of a line, as `bytes.TrimSpace` does.
fn trim(line: &[u8]) -> &[u8] {
    let start = line.iter().position(|byte| !byte.is_ascii_whitespace());
    let Some(start) = start else { return &[] };
    let end = line
        .iter()
        .rposition(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(start);
    &line[start..=end]
}

/// Clears the card values that do not fit the kind of their property, as a copy of
/// stored cards does: an archive holds what its board held, and a change of the
/// definitions leaves such values behind (`docs/contract-pending.md` 2).
fn clear_values_of_another_kind(boards_and_blocks: &mut BoardsAndBlocks) {
    let definitions: HashMap<&str, &[JsonMap]> = boards_and_blocks
        .boards
        .iter()
        .map(|board| (board.id.as_str(), board.card_properties.as_slice()))
        .collect();
    for block in &mut boards_and_blocks.blocks {
        let Some(definitions) = definitions.get(block.board_id.as_str()) else {
            continue;
        };
        if let Some(Value::Object(values)) = block.fields.get_mut("properties") {
            fvoci_model::clear_values_of_another_kind(values, definitions);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(id: &str, parent_id: &str) -> Block {
        Block {
            id: id.to_owned(),
            parent_id: parent_id.to_owned(),
            ..Block::default()
        }
    }

    fn ids_after_dropping(blocks: Vec<Block>) -> (Vec<String>, usize) {
        let mut archive = BoardsAndBlocks {
            boards: vec![Board {
                id: "board".to_owned(),
                ..Board::default()
            }],
            blocks,
        };
        let dropped = drop_orphans(&mut archive);
        let ids = archive.blocks.into_iter().map(|block| block.id).collect();
        (ids, dropped)
    }

    #[test]
    fn blocks_that_nothing_leads_to_are_dropped_with_what_is_below_them() {
        let (kept, dropped) = ids_after_dropping(vec![
            block("text", "card"),
            block("card", "board"),
            block("view", ""),
            block("lost", "gone"),
            block("below-lost", "lost"),
            block("deeper", "below-lost"),
            block("ring-a", "ring-b"),
            block("ring-b", "ring-a"),
            block("own", "own"),
            block("reply", "text"),
        ]);
        assert_eq!(
            kept,
            ["text", "card", "view", "reply"],
            "what a board leads to stays, in the order it came"
        );
        assert_eq!(
            dropped, 6,
            "the lost ones, and the rings that hang off nothing"
        );
    }

    /// A chain as long as an archive may be: the walk is one pass, where taking the
    /// orphans away level by level took one pass per level.
    #[test]
    fn a_long_chain_of_orphans_is_dropped_in_one_walk() {
        let length = 200_000;
        let mut blocks = vec![block("0", "missing")];
        blocks.extend((1..length).map(|index| block(&index.to_string(), &(index - 1).to_string())));
        blocks.push(block("card", "board"));

        let started = std::time::Instant::now();
        let (kept, dropped) = ids_after_dropping(blocks);

        assert_eq!(kept, ["card"]);
        assert_eq!(dropped, length);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "took {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn clean_dir_resolves_dot_segments() {
        assert_eq!(clean_dir("board1"), "board1", "a plain directory");
        assert_eq!(clean_dir("board1/../.."), "..", "traversal beyond the root");
        assert_eq!(clean_dir("a/./b"), "a/b", "a current-directory segment");
    }

    #[test]
    fn split_entry_name_takes_the_base_name() {
        assert_eq!(
            split_entry_name("board1/../../x"),
            ("..".to_owned(), "x".to_owned()),
            "the file name is the last segment"
        );
        assert_eq!(
            split_entry_name("version.json"),
            (".".to_owned(), "version.json".to_owned()),
            "a bare name is in the current directory"
        );
    }
}
