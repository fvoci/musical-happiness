//! Importing boards from the version 2 archive format and the older line-based format.
//!
//! Ports `server/app/import.go`. An import generates fresh identifiers for everything,
//! fixes up references, converts a legacy board-as-block into a real board, stores the
//! files and adds the importing user as an admin.

use std::collections::{HashMap, HashSet};
use std::io::{Cursor, Read as _};

use bytes::Bytes;
use fvoci_model::{
    ArchiveHeader, ArchiveLine, Block, BlockType, Board, BoardMember, BoardType, BoardsAndBlocks,
    BoardsAndBlocksError, JsonMap, renew_boards_and_blocks_ids,
};
use serde::Deserialize as _;
use serde_json::Value;

use super::App;
use super::export::ARCHIVE_VERSION;
use crate::auth::now_millis;
use crate::error::AppError;

/// The prefix of the older, line-based archive format.
const LEGACY_FILE_BEGIN: &[u8] = b"{\"version\":1";

/// The largest a single archive entry may decompress to. The original bounds only its
/// `board.jsonl`; this bounds every entry, so that a small upload cannot unfold into an
/// arbitrarily large file. It is a bound per entry, not for the archive as a whole.
const MAX_ENTRY_BYTES: u64 = 256 * 1024 * 1024;

/// The largest `board.jsonl`, or legacy archive, that is read: the original's
/// `importMaxFileSize` (`server/app/import.go:24`).
const MAX_JSONL_BYTES: usize = 70 * 1024 * 1024;

/// A board or block modifier applied during import: it may change the item and returns
/// whether to keep it. The set records the identifiers of skipped items.
pub(crate) type BoardModifier = Box<dyn Fn(&mut Board, &mut HashSet<String>) -> bool + Send + Sync>;

/// A block modifier; see [`BoardModifier`].
pub(crate) type BlockModifier = Box<dyn Fn(&mut Block, &mut HashSet<String>) -> bool + Send + Sync>;

/// How an archive is imported.
#[derive(Default)]
pub(crate) struct ImportOptions {
    /// The team the boards are imported into.
    pub(crate) team_id: String,
    /// The user who imports, recorded as the modifier and added as an admin.
    pub(crate) modified_by: String,
    /// An optional transform applied to each board, used when importing templates.
    pub(crate) board_modifier: Option<BoardModifier>,
    /// An optional transform applied to each block.
    pub(crate) block_modifier: Option<BlockModifier>,
}

impl std::fmt::Debug for ImportOptions {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ImportOptions")
            .field("team_id", &self.team_id)
            .field("modified_by", &self.modified_by)
            .field("board_modifier", &self.board_modifier.is_some())
            .field("block_modifier", &self.block_modifier.is_some())
            .finish()
    }
}

impl App {
    /// Imports an archive into a team on behalf of a user, with no modifiers. The entry
    /// point for the import route.
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
            ..ImportOptions::default()
        };
        self.import_archive(archive, &options).await
    }

    /// Imports an archive of zero or more boards with their blocks and files.
    ///
    /// A buffer that begins like the legacy line-based format is imported as a single
    /// board; anything else is read as a ZIP.
    ///
    /// # Errors
    ///
    /// Returns a server error if the buffer is not a valid archive, if its version is
    /// unsupported, or if a board cannot be stored.
    pub(crate) async fn import_archive(
        &self,
        archive: Bytes,
        options: &ImportOptions,
    ) -> Result<(), AppError> {
        if archive.starts_with(LEGACY_FILE_BEGIN) {
            tracing::debug!("importing legacy archive");
            self.import_board_jsonl(archive, options).await?;
            return Ok(());
        }

        let zip = zip::ZipArchive::new(Cursor::new(archive))
            .map_err(|error| AppError::bad_request(format_args!("{error}")))?;
        // The original's streaming reader rejects a ZIP with no entries as invalid.
        if zip.is_empty() {
            return Err(AppError::bad_request("zip: not a valid zip file"));
        }

        // First pass: the version and the boards; a file entry needs its board already
        // known.
        let mut board_map: HashMap<String, Board> = HashMap::new();
        for index in 0..zip.len() {
            let (dir, filename) = entry_name(&zip, index)?;
            match filename.as_str() {
                "version.json" => check_version(&read_entry(&zip, index).await?)?,
                "board.jsonl" => {
                    let bytes = read_entry(&zip, index).await?;
                    let board = self.import_board_jsonl(bytes, options).await?;
                    board_map.insert(dir, board);
                }
                // Files are read in the second pass only.
                _ => {}
            }
        }

        // Second pass: the files, placed under their board.
        let mut file_map: HashMap<String, String> = HashMap::new();
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
            file_map.insert(filename, new_name);
        }

        self.fix_images_attachments(&board_map, &file_map, &options.modified_by)
            .await;
        tracing::debug!(boards_imported = board_map.len(), "import archive - done");
        Ok(())
    }

    /// Rewrites every image and attachment block of each imported board to point at the
    /// newly stored file. Failures are logged, as in the original.
    async fn fix_images_attachments(
        &self,
        board_map: &HashMap<String, Board>,
        file_map: &HashMap<String, String>,
        user_id: &str,
    ) {
        for board in board_map.values() {
            let blocks = match self.store().get_blocks(&board_blocks(&board.id)).await {
                Ok(blocks) => blocks,
                Err(error) => {
                    tracing::info!(%error, board_id = board.id, "cannot retrieve imported blocks for board");
                    return;
                }
            };

            let mut block_ids = Vec::new();
            let mut block_patches = Vec::new();
            for block in &blocks {
                if block.block_type != BlockType::Image && block.block_type != BlockType::Attachment
                {
                    continue;
                }
                let old = block.fields.get("fileId").and_then(Value::as_str);
                let new_name = old.and_then(|old| file_map.get(old)).cloned();
                block_ids.push(block.id.clone());
                let mut updated_fields = JsonMap::new();
                updated_fields.insert(
                    "fileId".to_owned(),
                    Value::String(new_name.unwrap_or_default()),
                );
                block_patches.push(fvoci_model::BlockPatch {
                    updated_fields,
                    ..fvoci_model::BlockPatch::default()
                });
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
                tracing::info!(%error, "Error patching blocks for image import");
            }
        }
    }

    /// Imports one board's `board.jsonl` and returns the stored board.
    ///
    /// # Errors
    ///
    /// Returns a server error if a line is malformed or of an unknown type, or if the
    /// board cannot be stored.
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

        apply_modifiers(&mut boards_and_blocks, options);
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
            // What is imported on behalf of no user, the built-in templates, has no
            // administrator: nobody changes it.
            if !options.modified_by.is_empty() {
                let admin = BoardMember {
                    board_id: board.id.clone(),
                    user_id: options.modified_by.clone(),
                    scheme_admin: true,
                    ..BoardMember::default()
                };
                self.add_member_to_board(&admin).await?;
            }
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
            .ok_or_else(|| AppError::bad_request("missing board in archive: invalid board block"))
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
/// Returns a server error if a line is malformed or of an unknown type.
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
    let mut first_line = true;

    for (number, raw) in jsonl.split(|&byte| byte == b'\n').enumerate() {
        let line_number = number + 1;
        let line = trim(raw);
        if line.is_empty() {
            continue;
        }
        if first_line && line.starts_with(LEGACY_FILE_BEGIN) {
            // An old archive's header tag; skip it without ending the first line.
            continue;
        }

        let archive_line: ArchiveLine = serde_json::from_slice(line).map_err(|error| {
            AppError::bad_request(format_args!(
                "error parsing archive line {line_number}: {error}"
            ))
        })?;
        let line_type = if first_line && archive_line.line_type == "block" {
            // Legacy archives encode boards as blocks.
            "board_block"
        } else {
            archive_line.line_type.as_str()
        };

        match line_type {
            "board" => {
                let mut board: Board = serde_json::from_value(with_a_minimum_role(
                    archive_line.data,
                ))
                .map_err(|error| {
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
            "board_block" => {
                let mut block: Block =
                    serde_json::from_value(archive_line.data).map_err(|error| {
                        AppError::bad_request(format_args!(
                            "invalid board block in archive line {line_number}: {error}"
                        ))
                    })?;
                modifier_user.clone_into(&mut block.modified_by);
                block.update_at = now;
                let board = block_to_board(&block, team_id)?;
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
        first_line = false;
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
        let mut entry = zip
            .by_index(index)
            .map_err(|error| AppError::bad_request(format_args!("{error}")))?;
        let mut bytes = Vec::new();
        let read = entry
            .by_ref()
            .take(MAX_ENTRY_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| {
                AppError::bad_request(format_args!("cannot read archive entry: {error}"))
            })?;
        if u64::try_from(read).unwrap_or(u64::MAX) > MAX_ENTRY_BYTES {
            return Err(AppError::bad_request(
                "archive entry exceeds the size limit",
            ));
        }
        Ok(Bytes::from(bytes))
    })
    .await
    .map_err(|error| AppError::internal(format_args!("archive task failed: {error}")))?
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

/// Converts a legacy board-as-block into a real board.
///
/// # Errors
///
/// Returns a bad request if the block is not a board block.
fn block_to_board(block: &Block, team_id: &str) -> Result<Board, AppError> {
    if block.block_type != BlockType::Board {
        return Err(AppError::bad_request("block is not a board"));
    }
    let fields = &block.fields;
    Ok(Board {
        id: block.id.clone(),
        team_id: team_id.to_owned(),
        created_by: block.created_by.clone(),
        modified_by: block.modified_by.clone(),
        board_type: BoardType::Private,
        title: block.title.clone(),
        create_at: block.create_at,
        update_at: block.update_at,
        delete_at: block.delete_at,
        icon: string_field(fields, "icon"),
        description: string_field(fields, "description"),
        show_description: bool_field(fields, "showDescription"),
        is_template: bool_field(fields, "isTemplate"),
        // The original reads `templateVer` with a Go type assertion that never succeeds
        // on a JSON-decoded number, so the version is always left at zero here.
        properties: map_field(fields, "properties"),
        card_properties: array_of_maps_field(fields, "cardProperties"),
        ..Board::default()
    })
}

/// Applies the board and block modifiers, dropping the items they reject.
fn apply_modifiers(boards_and_blocks: &mut BoardsAndBlocks, options: &ImportOptions) {
    if options.board_modifier.is_none() && options.block_modifier.is_none() {
        return;
    }
    let mut cache: HashSet<String> = HashSet::new();
    if let Some(modifier) = &options.board_modifier {
        boards_and_blocks
            .boards
            .retain_mut(|board| modifier(board, &mut cache));
    }
    if let Some(modifier) = &options.block_modifier {
        boards_and_blocks
            .blocks
            .retain_mut(|block| modifier(block, &mut cache));
    }
}

/// The string value of a field, or the empty string.
fn string_field(fields: &JsonMap, key: &str) -> String {
    fields
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// The boolean value of a field, or false.
fn bool_field(fields: &JsonMap, key: &str) -> bool {
    fields.get(key).and_then(Value::as_bool).unwrap_or(false)
}

/// The object value of a field, or an empty map.
fn map_field(fields: &JsonMap, key: &str) -> JsonMap {
    match fields.get(key) {
        Some(Value::Object(map)) => map.clone(),
        _ => JsonMap::new(),
    }
}

/// The array-of-objects value of a field, or an empty vector. A non-object element makes
/// the whole value empty, as in the original.
fn array_of_maps_field(fields: &JsonMap, key: &str) -> Vec<JsonMap> {
    let Some(Value::Array(array)) = fields.get(key) else {
        return Vec::new();
    };
    let mut maps = Vec::with_capacity(array.len());
    for element in array {
        match element {
            Value::Object(map) => maps.push(map.clone()),
            _ => return Vec::new(),
        }
    }
    maps
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

/// A board of an archive, with its `minimumRole` left out unless it is one a board can
/// have here, so that the board gets the default. Archives written by the original say
/// `""` for "none", which is not a minimum role any more, and may say `admin`, which
/// is none either.
fn with_a_minimum_role(mut board: Value) -> Value {
    if let Some(members) = board.as_object_mut() {
        let is_one = members
            .get("minimumRole")
            .is_some_and(|role| fvoci_model::MinimumRole::deserialize(role).is_ok());
        if !is_one {
            members.remove("minimumRole");
        }
    }
    board
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
