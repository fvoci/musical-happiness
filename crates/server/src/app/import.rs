//! Importing boards from the archive format of version 3 (`super::export`).
//!
//! Ports `server/app/import.go`. An import generates fresh identifiers for everything,
//! fixes up references, stores the files, points the blocks at them and adds the
//! importing user as an admin. The formats of the original (the line-based version 1
//! and the ZIP of version 2) are refused (`docs/s6-design.md` 3.10).

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::io::{self, Read, Seek, SeekFrom, Write as _};
use std::path::Path;
use std::sync::Arc;

use bytes::Bytes;
use fvoci_model::{
    ArchiveHeader, ArchiveLine, Block, BlockType, Board, BoardMember, BoardsAndBlocks,
    BoardsAndBlocksError, JsonMap, renew_boards_and_blocks_ids,
};
use serde_json::Value;
use tokio::fs;

use super::App;
use super::export::ARCHIVE_VERSION;
use crate::auth::now_millis;
use crate::error::{AppError, ErrorKind};
use fvoci_store::StoreError;

/// The most an archive's entries may decompress to together (`docs/s6-design.md` 4.2):
/// what keeps a small archive from unfolding into a disk's worth.
const MAX_UNPACKED_BYTES: u64 = 1024 * 1024 * 1024;

/// The most entries an archive has (`docs/s6-design.md` 4.2).
const MAX_ENTRIES: u64 = 10_000;

/// The signature each entry of a ZIP's central directory begins with (APPNOTE 4.3.12).
const CENTRAL_HEADER: &[u8; 4] = b"PK\x01\x02";

/// How much of an archive is read at a time to count its central headers.
const SCAN_CHUNK_BYTES: usize = 1024 * 1024;

/// The largest `board.jsonl` that is read (`docs/s6-design.md` 4.2).
const MAX_JSONL_BYTES: u64 = 64 * 1024 * 1024;

/// The largest `version.json` that is read: it is one small object.
const MAX_VERSION_BYTES: u64 = 64 * 1024;

/// How an archive is imported.
#[derive(Debug, Default)]
pub(crate) struct ImportOptions {
    /// The team the boards are imported into.
    pub(crate) team_id: String,
    /// The user who imports, recorded as the modifier and added as an admin.
    pub(crate) modified_by: String,
}

impl App {
    /// Imports the archive in the file at `path` into a team on behalf of a user. The
    /// entry point for the import of an upload.
    ///
    /// # Errors
    ///
    /// Returns the errors of [`App::import_archive`].
    pub(crate) async fn import_team_archive(
        &self,
        path: &Path,
        team_id: &str,
        modified_by: &str,
    ) -> Result<(), AppError> {
        let options = ImportOptions {
            team_id: team_id.to_owned(),
            modified_by: modified_by.to_owned(),
        };
        self.import_archive(path, &options).await
    }

    /// Imports the archive in the file at `path`, of zero or more boards with their
    /// blocks and files. The archive is read from the file at random, an entry at a time;
    /// a file entry is unpacked into a file beside it, `path` with the name `entry`, to be
    /// stored from there.
    ///
    /// # Errors
    ///
    /// Returns a bad request if the file is not a ZIP, has no `version.json` or one of
    /// another version, or holds what does not fit together; a 413 for an archive of
    /// more than 10,000 entries, whose entries unpack to more than 1 GiB together, or
    /// with a `board.jsonl` of more than 64 MiB or a file larger than `maxfilesize`; and
    /// the store's error if a board cannot be stored.
    pub(crate) async fn import_archive(
        &self,
        path: &Path,
        options: &ImportOptions,
    ) -> Result<(), AppError> {
        let zip = open_archive(path).await?;
        let unpacked_file = path.with_file_name("entry");
        // The original's streaming reader rejects a ZIP with no entries as invalid.
        if zip.is_empty() {
            return Err(AppError::bad_request("zip: not a valid zip file"));
        }
        // Not reached while the headers are counted before the directory is read (see
        // `open_archive`); kept as the check of what was read.
        if u64::try_from(zip.len()).map_or(true, |entries| entries > MAX_ENTRIES) {
            return Err(too_many_entries());
        }
        let largest_file = u64::try_from(self.largest_upload()).unwrap_or(0);

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
        // A `version.json` too large to be one is no archive of this server's, as one of
        // another version is not.
        let version = read_entry(&zip, version, MAX_VERSION_BYTES)
            .await
            .map_err(|error| {
                if error.kind() == ErrorKind::PayloadTooLarge {
                    AppError::bad_request("version.json is not that of an archive")
                } else {
                    error
                }
            })?;
        check_version(&version)?;
        // Every entry is read through once before anything is stored: an entry that is
        // broken or too large, or all of them unpacking to too much, refuses the archive
        // while nothing of it is stored yet.
        let mut unpacked: u64 = 0;
        for index in 0..zip.len() {
            let (dir, filename) = entry_name(&zip, index)?;
            let limit = entry_limit(&dir, &filename, largest_file);
            unpacked = unpacked.saturating_add(check_entry(&zip, index, limit).await?);
            if unpacked > MAX_UNPACKED_BYTES {
                return Err(AppError::too_large(
                    format_args!(
                        "the archive's entries unpack to more than {MAX_UNPACKED_BYTES} bytes"
                    ),
                    MAX_UNPACKED_BYTES,
                ));
            }
        }

        // Every board is read and checked as the store checks it before the first is
        // stored: an archive that one of them refuses stores nothing. What is read is
        // dropped and read again to be stored, so that one board at a time is held.
        let mut boards = Vec::new();
        for index in 0..zip.len() {
            let (dir, filename) = entry_name(&zip, index)?;
            if filename == "board.jsonl" {
                boards.push((index, dir));
            }
        }
        for (index, _) in &boards {
            parse_board_jsonl(read_entry(&zip, *index, MAX_JSONL_BYTES).await?, options).await?;
        }

        // First pass: the boards; a file entry needs its board already known.
        let mut board_map: HashMap<String, Board> = HashMap::new();
        for (index, dir) in boards {
            let jsonl = read_entry(&zip, index, MAX_JSONL_BYTES).await?;
            let board = parse_board_jsonl(jsonl, options).await?;
            let board = self.store_archive_board(board, options).await?;
            board_map.insert(dir, board);
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
            let saved = match unpack_entry(&zip, index, largest_file, &unpacked_file).await {
                Ok(()) => match fs::File::open(&unpacked_file).await {
                    Ok(mut file) => self.save_file(&mut file, &board.id, &filename).await,
                    Err(error) => Err(AppError::internal(format_args!(
                        "cannot open an unpacked archive entry: {error}"
                    ))),
                },
                Err(error) => Err(error),
            };
            remove_unpacked(&unpacked_file).await;
            let new_name = saved.map_err(|error| {
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
                    tracing::error!(
                        %error,
                        board_id = board.id,
                        "cannot retrieve imported blocks for board"
                    );
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
                    } else if block.block_type == BlockType::Attachment {
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
                tracing::error!(
                    %error,
                    board_id = board.id,
                    "the imported blocks could not be pointed at their files"
                );
            }
        }
    }

    /// Stores one board of an archive as [`parse_board_jsonl`] read it, with its
    /// members who are users of this server and the importing user as an admin, and
    /// returns the stored board.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the board or a membership cannot be stored. What
    /// the store refuses as a bad request, [`parse_board_jsonl`] has refused before.
    async fn store_archive_board(
        &self,
        board: ParsedBoard,
        options: &ImportOptions,
    ) -> Result<Board, AppError> {
        let ParsedBoard {
            boards_and_blocks,
            members,
        } = board;
        // Keep only members who are users of this server.
        let mut kept_members = Vec::with_capacity(members.len());
        for member in members {
            if self.store().get_user_by_id(&member.user_id).await.is_ok() {
                kept_members.push(member);
            }
        }

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

/// One board of an archive, read and checked, with new identifiers, before it is
/// stored.
struct ParsedBoard {
    boards_and_blocks: BoardsAndBlocks,
    members: Vec<BoardMember>,
}

/// Reads and checks one board's `board.jsonl`: its lines, the types of its blocks,
/// whether its boards and blocks fit together, and each board and block as the store
/// checks it (the card property definitions of a board; the title, the fields with the
/// limit of their type, the property values and the body of a block). Values of the
/// other kind are cleared, blocks that nothing leads to are dropped, and everything gets
/// a new identifier.
///
/// What the store still checks against what is stored is the parent of a block: a
/// parent that is a board of the same `board.jsonl` other than the block's own. An
/// archive of this server has one board per `board.jsonl`.
///
/// # Errors
///
/// Returns a bad request if a line is malformed or of an unknown
/// type, a block has a type that no new block has, the boards and blocks do not fit
/// together, or a board or block is not one the store takes, with the store's message.
async fn parse_board_jsonl(jsonl: Bytes, options: &ImportOptions) -> Result<ParsedBoard, AppError> {
    // Parsing tens of megabytes of JSON would hold up a worker of the runtime.
    let (team_id, modified_by) = (options.team_id.clone(), options.modified_by.clone());
    let (mut boards_and_blocks, members) =
        tokio::task::spawn_blocking(move || parse_archive_lines(&jsonl, &team_id, &modified_by))
            .await
            .map_err(|error| AppError::internal(format_args!("archive task failed: {error}")))??;

    // An archive of this server may hold blocks stored before a card's content became
    // its body; none is made again (`docs/contract-pending.md` 1).
    if let Some(block) = boards_and_blocks
        .blocks
        .iter()
        .find(|block| !block.block_type.is_current())
    {
        return Err(AppError::bad_request(format_args!(
            "the archive's block {}: {}",
            block.id,
            fvoci_model::BlockError::type_not_current(&block.block_type)
        )));
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
        // A board without blocks is a board all the same, and what exporting one gives.
        // The original refuses such an archive.
        Ok(()) | Err(BoardsAndBlocksError::NoBlocks) => {
            renew_boards_and_blocks_ids(boards_and_blocks)
        }
        Err(error) => {
            return Err(AppError::bad_request(format_args!(
                "the archive's boards and blocks do not fit together: {error}"
            )));
        }
    };
    // As the store refuses them, with its messages, before anything is stored.
    for board in &boards_and_blocks.boards {
        board
            .validate_card_properties()
            .map_err(|error| AppError::from(StoreError::InvalidBoard(error.into())))?;
    }
    for block in &boards_and_blocks.blocks {
        block.validate().map_err(|source| {
            AppError::from(StoreError::InvalidBlock {
                id: block.id.clone(),
                source,
            })
        })?;
    }
    Ok(ParsedBoard {
        boards_and_blocks,
        members,
    })
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

/// The archive as it is read while it is imported. Cloning it copies no content.
type Archive = zip::ZipArchive<SharedFile>;

/// A file read at positions of its own: each clone has its own position over the one
/// open file, so that the archive can be cloned into each task that reads an entry.
#[derive(Debug, Clone)]
struct SharedFile {
    file: Arc<std::fs::File>,
    len: u64,
    position: u64,
}

impl Read for SharedFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        #[cfg(unix)]
        let read = std::os::unix::fs::FileExt::read_at(&*self.file, buf, self.position)?;
        #[cfg(windows)]
        let read = std::os::windows::fs::FileExt::seek_read(&*self.file, buf, self.position)?;
        #[cfg(not(any(unix, windows)))]
        compile_error!("an archive is read at positions, which unix and windows have");
        self.position = self
            .position
            .saturating_add(u64::try_from(read).unwrap_or(u64::MAX));
        Ok(read)
    }
}

impl Seek for SharedFile {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let position = match to {
            SeekFrom::Start(offset) => Some(offset),
            SeekFrom::End(offset) => self.len.checked_add_signed(offset),
            SeekFrom::Current(offset) => self.position.checked_add_signed(offset),
        };
        self.position = position.ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "a seek before the start")
        })?;
        Ok(self.position)
    }
}

/// Opens the archive in the file at `path` and reads its directory, off the runtime's
/// workers.
async fn open_archive(path: &Path) -> Result<Archive, AppError> {
    let path = path.to_owned();
    tokio::task::spawn_blocking(move || {
        let file = std::fs::File::open(&path).map_err(|error| {
            AppError::internal(format_args!("cannot open the archive: {error}"))
        })?;
        let len = file
            .metadata()
            .map_err(|error| AppError::internal(format_args!("cannot read the archive: {error}")))?
            .len();
        let mut file = SharedFile {
            file: Arc::new(file),
            len,
            position: 0,
        };
        let mut start = [0; 12];
        let read = file.read(&mut start).unwrap_or(0);
        if start
            .get(..read)
            .is_some_and(|start| start.starts_with(br#"{"version":1"#))
        {
            return Err(AppError::bad_request(format_args!(
                "an archive of version 1 is not read; this server reads version {ARCHIVE_VERSION}"
            )));
        }
        // Counted before the directory is read, as reading it holds every entry in memory.
        // Every entry the reader keeps begins with the central header's signature, so it
        // keeps at most as many entries as signatures, whatever the end records say. The
        // capacity it reserves first follows the declared count, which the crate bounds
        // only by the file's length (about a 47th of it, as entries): that reservation is
        // not bounded here (`docs/contract-pending.md` 5).
        file.position = 0;
        let headers = count_central_headers(&mut file, MAX_ENTRIES).map_err(|error| {
            AppError::internal(format_args!("cannot read the archive: {error}"))
        })?;
        if headers > MAX_ENTRIES {
            return Err(too_many_entries());
        }
        file.position = 0;
        zip::ZipArchive::new(file).map_err(|error| AppError::bad_request(format_args!("{error}")))
    })
    .await
    .map_err(|error| AppError::internal(format_args!("archive task failed: {error}")))?
}

/// The refusal of an archive of more than [`MAX_ENTRIES`] entries.
fn too_many_entries() -> AppError {
    AppError::too_large(
        format_args!("the archive has more than {MAX_ENTRIES} entries"),
        MAX_ENTRIES,
    )
}

/// Counts the central headers' signatures in what `reader` gives, up to the first count
/// over `limit`, where it stops reading.
fn count_central_headers(reader: &mut impl Read, limit: u64) -> io::Result<u64> {
    let mut chunk = vec![0; SCAN_CHUNK_BYTES];
    // The last bytes of the chunk before: a signature may span two chunks.
    let mut carried = 0;
    let mut count: u64 = 0;
    loop {
        let read = match reader.read(chunk.get_mut(carried..).unwrap_or_default()) {
            Ok(0) => return Ok(count),
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        let filled = chunk.get(..carried + read).unwrap_or_default();
        let found = filled
            .windows(CENTRAL_HEADER.len())
            .filter(|window| window == CENTRAL_HEADER)
            .count();
        count = count.saturating_add(u64::try_from(found).unwrap_or(u64::MAX));
        if count > limit {
            return Ok(count);
        }
        // Three bytes are kept: four would count a signature at the end twice.
        let keep = filled.len().min(CENTRAL_HEADER.len() - 1);
        let start = filled.len() - keep;
        chunk.copy_within(start..start + keep, 0);
        carried = keep;
    }
}

/// The most an entry may unpack to: a `board.jsonl`, the archive's `version.json` (the
/// one at its root), or a file, which is held to the limit of an upload.
fn entry_limit(dir: &str, filename: &str, largest_file: u64) -> u64 {
    match filename {
        "board.jsonl" => MAX_JSONL_BYTES,
        "version.json" if dir == "." => MAX_VERSION_BYTES,
        _ => largest_file,
    }
}

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

/// Reads the content of an archive entry, at most `limit` bytes of it. Decompression is
/// done off the runtime's workers, which it would hold up.
async fn read_entry(zip: &Archive, index: usize, limit: u64) -> Result<Bytes, AppError> {
    let mut zip = zip.clone();
    tokio::task::spawn_blocking(move || {
        let mut bytes = Vec::new();
        decompress(&mut zip, index, limit, &mut bytes)?;
        Ok(Bytes::from(bytes))
    })
    .await
    .map_err(|error| AppError::internal(format_args!("archive task failed: {error}")))?
}

/// Unpacks an archive entry, at most `limit` bytes of it, into the file `to`, off the
/// runtime's workers.
async fn unpack_entry(zip: &Archive, index: usize, limit: u64, to: &Path) -> Result<(), AppError> {
    let mut zip = zip.clone();
    let to = to.to_owned();
    tokio::task::spawn_blocking(move || {
        let file = std::fs::File::create(&to).map_err(|error| {
            AppError::internal(format_args!("cannot unpack an archive entry: {error}"))
        })?;
        let mut out = io::BufWriter::new(file);
        decompress(&mut zip, index, limit, &mut out)?;
        out.flush().map_err(|error| {
            AppError::internal(format_args!("cannot unpack an archive entry: {error}"))
        })
    })
    .await
    .map_err(|error| AppError::internal(format_args!("archive task failed: {error}")))?
}

/// Removes an unpacked entry once it is stored, or failed to be.
async fn remove_unpacked(path: &Path) {
    match fs::remove_file(path).await {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => tracing::error!(%error, "an unpacked archive entry could not be removed"),
    }
}

/// Reads an archive entry through without keeping it, as [`read_entry`] would read it:
/// its checksum and its bound are checked. Returns how much it unpacks to.
async fn check_entry(zip: &Archive, index: usize, limit: u64) -> Result<u64, AppError> {
    let mut zip = zip.clone();
    tokio::task::spawn_blocking(move || decompress(&mut zip, index, limit, &mut io::sink()))
        .await
        .map_err(|error| AppError::internal(format_args!("archive task failed: {error}")))?
}

/// Decompresses an archive entry into `out`, at most `limit` bytes of it, and returns
/// how many. The reader checks the entry's checksum when it reaches the end.
fn decompress(
    zip: &mut Archive,
    index: usize,
    limit: u64,
    out: &mut impl io::Write,
) -> Result<u64, AppError> {
    let mut entry = zip
        .by_index(index)
        .map_err(|error| AppError::bad_request(format_args!("{error}")))?;
    let name = entry.name().map(Cow::into_owned).unwrap_or_default();
    let read =
        io::copy(&mut entry.by_ref().take(limit.saturating_add(1)), out).map_err(|error| {
            AppError::bad_request(format_args!("cannot read archive entry: {error}"))
        })?;
    if read > limit {
        return Err(AppError::too_large(
            format_args!("the archive entry {name} unpacks to more than {limit} bytes"),
            limit,
        ));
    }
    Ok(read)
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

    /// The central headers are counted wherever they are, a chunk's end included, and
    /// the count stops once it is over the limit.
    #[test]
    fn central_headers_are_counted_across_chunks() {
        let count = |bytes: &[u8], limit: u64| {
            count_central_headers(&mut &bytes[..], limit).expect("a slice reads")
        };
        assert_eq!(count(b"", 10), 0, "nothing");
        assert_eq!(count(b"PK\x01", 10), 0, "a part of one");
        assert_eq!(count(b"PK\x01\x02PK\x01\x02xPK\x01\x02", 10), 3, "three");
        for at in SCAN_CHUNK_BYTES - 3..=SCAN_CHUNK_BYTES {
            let mut bytes = vec![0; SCAN_CHUNK_BYTES * 2];
            bytes[at..at + 4].copy_from_slice(CENTRAL_HEADER);
            assert_eq!(count(&bytes, 10), 1, "one at {at}, on the chunk's edge");
        }
        let many = CENTRAL_HEADER.repeat(SCAN_CHUNK_BYTES);
        let in_a_chunk = u64::try_from(SCAN_CHUNK_BYTES / 4).expect("a small count");
        assert_eq!(count(&many, 5), in_a_chunk, "stops after the chunk");
    }

    /// A reader that gives a few bytes at a time, as a file may.
    struct Trickle<'b>(&'b [u8], usize);

    impl Read for Trickle<'_> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let n = self.1.min(buf.len()).min(self.0.len());
            let (head, rest) = self.0.split_at(n);
            buf.get_mut(..n).unwrap_or_default().copy_from_slice(head);
            self.0 = rest;
            Ok(n)
        }
    }

    /// Short reads count as whole ones: a signature split between reads is found once.
    #[test]
    fn central_headers_are_counted_over_short_reads() {
        let mut bytes = Vec::new();
        for gap in 0..200 {
            bytes.extend_from_slice(CENTRAL_HEADER);
            bytes.extend(std::iter::repeat_n(b'x', gap % 9));
        }
        let expected = u64::try_from(
            bytes
                .windows(4)
                .filter(|window| window == CENTRAL_HEADER)
                .count(),
        )
        .expect("a small count");
        for step in [1, 2, 3, 5, 7, 4096] {
            let counted = count_central_headers(&mut Trickle(&bytes, step), u64::MAX)
                .expect("a reader that does not fail");
            assert_eq!(counted, expected, "{step} bytes a read");
        }
    }

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
