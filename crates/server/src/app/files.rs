//! Files attached to boards: storing uploads, resolving their paths and copying them
//! when cards are duplicated.
//!
//! Ports `server/app/files.go`. A template upload keeps its name under
//! `<team id>/<board id>/<name>`, as in the original. A normal upload goes to
//! `boards/YYYYMMDD/<board id>/<generated id><ext>`; the original has no board in that
//! path (`boards/YYYYMMDD/<generated id><ext>`). The path of every file is recorded in
//! its file-info row and read from there, so files stored either way are found by both
//! servers.
//!
//! The board in the path is what ties a file to its board. The original does not tie
//! them at all: whoever may read any board can fetch any file whose name they know
//! through that board. Here a file is given out, copied and exported only through the
//! board its path names, in every layout; a path that names no board, such as the
//! original's `boards/YYYYMMDD/<name>`, is given out through none.

use fvoci_model::{
    Block, BlockPatch, BlockPatchBatch, BlockType, Board, FileInfo, IdKind, JsonMap, new_id,
};
use fvoci_store::DuplicateAs;
use serde_json::Value;
use tokio::io::AsyncRead;

use super::App;
use crate::auth::now_millis;
use crate::error::AppError;
use crate::storage::StorageError;

/// The marker the original writes in a file-info row whose path is unknown; such a path
/// is treated as absent (`server/app/files.go:18`).
const EMPTY_PATH: &str = "empty";

/// What a request misses that names a file that is not there, or not the board's.
pub(crate) const FILE: &str = "file";

/// The directory normal uploads are stored under.
const UPLOADS_ROOT: &str = "boards";

impl App {
    /// Stores an uploaded file and records it, and returns the name the client uses to
    /// fetch it back.
    ///
    /// The name comes from the client and is reduced to its base name first, so a name
    /// with path separators cannot reach the storage layer. A normal upload is stored
    /// under a generated name in today's directory; a template upload keeps its name.
    ///
    /// # Errors
    ///
    /// Returns the storage error if the file cannot be written (wrapped as the original
    /// does), or the store's error if the record cannot be saved.
    pub async fn save_file(
        &self,
        reader: &mut (dyn AsyncRead + Send + Unpin),
        team_id: &str,
        board_id: &str,
        filename: &str,
        as_template: DuplicateAs,
    ) -> Result<String, AppError> {
        let base = base_name(filename);
        let extension = stored_extension(base);
        let upload_extension = upload_extension(&extension);

        let created = new_id(IdKind::None);
        let new_file_name = match as_template {
            DuplicateAs::Template => base.to_owned(),
            DuplicateAs::Copy => format!("{created}{upload_extension}"),
        };
        let path = destination_path(as_template, team_id, board_id, &new_file_name);

        let size = self.files().write(&path, reader).await.map_err(|error| {
            AppError::internal(format_args!(
                "unable to store the file in the files storage: {error}"
            ))
        })?;

        let info = FileInfo {
            id: file_info_id(&created)?,
            path,
            size: size_to_i64(size),
            name: base.to_owned(),
            extension,
            create_at: now_millis(),
            update_at: now_millis(),
            ..FileInfo::default()
        };
        self.store().save_file_info(&info).await?;
        Ok(new_file_name)
    }

    /// Returns the record of the file a client names, or `None` if there is none.
    ///
    /// The name is in the form `<kind char><id>.<ext>`; the identifier is the part
    /// between the kind character and the first dot.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if the name is one no file can have (empty, or beginning
    /// with a dot), or the store's error.
    pub async fn get_file_info(&self, filename: &str) -> Result<Option<FileInfo>, AppError> {
        if filename.is_empty() {
            return Err(AppError::not_found(FILE));
        }
        let stem = filename.split('.').next().unwrap_or_default();
        if stem.is_empty() {
            return Err(AppError::not_found(FILE));
        }
        // The original drops the first byte of the name. Where that falls inside a
        // character of several bytes, what is left names no record.
        let Some(id) = stem.get(1..) else {
            return Ok(None);
        };
        match self.store().get_file_info(id).await {
            Ok(info) => Ok(Some(info)),
            Err(error) if error.is_not_found() => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    /// Returns the record of a file and the path it is stored at. When there is no
    /// record, or its path is unset, the path is built from the team, the board and the
    /// name, which is where files were kept before the file-info table existed.
    ///
    /// # Errors
    ///
    /// Returns the errors of [`App::get_file_info`].
    pub async fn get_file_path(
        &self,
        team_id: &str,
        board_id: &str,
        filename: &str,
    ) -> Result<(Option<FileInfo>, String), AppError> {
        let info = self.get_file_info(filename).await?;
        let path = match &info {
            Some(info) if !info.path.is_empty() && info.path != EMPTY_PATH => info.path.clone(),
            _ => join3(team_id, board_id, filename),
        };
        Ok((info, path))
    }

    /// Opens a stored file of a board for reading, or returns `None` if it does not exist
    /// at its resolved path or belongs to another board. Used by the archive export,
    /// which skips a missing file.
    ///
    /// # Errors
    ///
    /// Returns the errors of [`App::get_file_path`], or the storage error if the file
    /// cannot be opened.
    pub(crate) async fn open_stored_file(
        &self,
        team_id: &str,
        board_id: &str,
        filename: &str,
    ) -> Result<Option<crate::storage::FileStream>, AppError> {
        let (_info, path) = self.get_file_path(team_id, board_id, filename).await?;
        if !belongs_to_board(&path, board_id) {
            tracing::warn!(board_id, "not reading a file that belongs to another board");
            return Ok(None);
        }
        if !self.files().exists(&path).await? {
            return Ok(None);
        }
        Ok(Some(self.files().read(&path, None).await?))
    }

    /// Gives duplicated image and attachment blocks files of their own: copies each file
    /// the blocks name and writes the names of the copies into the stored blocks.
    ///
    /// Called by board and block duplication and by block insertion with a
    /// `sourceBoardID`. The blocks passed in are not changed; callers that go on to use
    /// them read them again or, on insertion, store them as they are.
    ///
    /// A failure to copy is logged and does not fail the call; the blocks then end up
    /// naming no file.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the new file names cannot be written to the blocks.
    pub async fn copy_and_update_card_files(
        &self,
        board_id: &str,
        user_id: &str,
        blocks: &[Block],
        duplicate_as: DuplicateAs,
    ) -> Result<(), AppError> {
        // As in the original, a failure to copy does not stop the duplication: it is
        // logged, and the copied blocks end up naming no file
        // (`server/app/files.go:186-190`).
        let new_file_names = match self.copy_card_files(board_id, blocks, duplicate_as).await {
            Ok(names) => names,
            Err(error) => {
                tracing::error!(board_id, %error, "Could not copy files while duplicating board");
                std::collections::HashMap::new()
            }
        };

        // Write the names of the copies to the stored blocks.
        let mut block_ids = Vec::new();
        let mut block_patches = Vec::new();
        for block in blocks {
            if block.block_type != BlockType::Image && block.block_type != BlockType::Attachment {
                continue;
            }
            let Some(Value::String(file_id)) = block.fields.get("fileId") else {
                continue;
            };
            let new_name = new_file_names.get(file_id).cloned().unwrap_or_default();
            block_ids.push(block.id.clone());
            let mut updated_fields = JsonMap::new();
            updated_fields.insert("fileId".to_owned(), Value::String(new_name));
            block_patches.push(BlockPatch {
                updated_fields,
                deleted_fields: vec!["attachmentId".to_owned()],
                ..BlockPatch::default()
            });
        }

        if !block_ids.is_empty() {
            let batch = BlockPatchBatch {
                block_ids,
                block_patches,
            };
            self.store()
                .patch_blocks(&batch, user_id)
                .await
                .map_err(|error| {
                    AppError::from(error).context(&format!(
                        "could not patch file IDs while duplicating board {board_id}"
                    ))
                })?;
        }
        Ok(())
    }

    /// Copies the file of every image and attachment block to a new file under the
    /// destination board's path. Returns a map from each old file name to the name of
    /// its copy; the blocks are not changed.
    ///
    /// A file that cannot be copied is logged and skipped, as in the original, so a
    /// block whose source file is missing still gets a fresh record.
    async fn copy_card_files(
        &self,
        source_board_id: &str,
        blocks: &[Block],
        duplicate_as: DuplicateAs,
    ) -> Result<std::collections::HashMap<String, String>, AppError> {
        let source_board = self.board(source_board_id).await?;

        let mut new_file_names = std::collections::HashMap::new();
        let mut destination: Option<Board> = None;
        for block in blocks {
            if block.block_type != BlockType::Image && block.block_type != BlockType::Attachment {
                continue;
            }
            let Some(file_id) = file_reference(&block.fields) else {
                continue;
            };

            let destination_board = if block.board_id == source_board.id {
                &source_board
            } else {
                if destination
                    .as_ref()
                    .is_none_or(|board: &Board| board.id != block.board_id)
                {
                    destination = Some(self.board(&block.board_id).await?);
                }
                destination
                    .as_ref()
                    .expect("the destination board was fetched just above")
            };

            let extension = stored_extension(base_name(&file_id));
            let dest_file_info_id = new_id(IdKind::None);
            let dest_filename = format!("{dest_file_info_id}{extension}");

            let (info, source_path) = self
                .get_file_path(&source_board.team_id, &source_board.id, &file_id)
                .await?;
            // A block can name any file. One that belongs to another board is not copied
            // into this one; the copied block ends up naming no file.
            if !belongs_to_board(&source_path, &source_board.id) {
                tracing::warn!(
                    source_board_id,
                    block_id = %block.id,
                    "not copying a file that belongs to another board"
                );
                continue;
            }
            let destination_path = destination_path(
                duplicate_as,
                &destination_board.team_id,
                &destination_board.id,
                &dest_filename,
            );

            let mut info = info.unwrap_or_else(|| new_file_info(&dest_filename));
            info.id = file_info_id(&dest_file_info_id)?;
            info.path.clone_from(&destination_path);
            self.store().save_file_info(&info).await?;

            if let Err(error) = self.files().copy(&source_path, &destination_path).await {
                tracing::error!(
                    %error,
                    source_path,
                    destination_path,
                    "CopyCardFiles failed to copy file"
                );
            }

            // The original leaves the blocks unchanged here and only returns the name
            // map; the pointer change is written to the database by the caller.
            new_file_names.insert(file_id, dest_filename);
        }
        Ok(new_file_names)
    }
}

/// The `fileId`, or failing that the `attachmentId`, of a block's fields, when it is a
/// string.
fn file_reference(fields: &JsonMap) -> Option<String> {
    for key in ["fileId", "attachmentId"] {
        if let Some(Value::String(value)) = fields.get(key) {
            return Some(value.clone());
        }
    }
    None
}

/// The last path segment of a name, splitting on `/` as the original's multipart reader
/// does. A name with no separator is returned unchanged.
pub(crate) fn base_name(name: &str) -> &str {
    name.rsplit('/').next().unwrap_or(name)
}

/// The lower-cased extension of a name, including the leading dot, or the empty string.
fn stored_extension(name: &str) -> String {
    match name.rfind('.') {
        Some(index) => name[index..].to_lowercase(),
        None => String::new(),
    }
}

/// The extension a generated upload name gets: the stored extension, but `.jpeg`
/// normalised to `.jpg` (`server/app/files.go:25`).
fn upload_extension(extension: &str) -> String {
    if extension == ".jpeg" {
        ".jpg".to_owned()
    } else {
        extension.to_owned()
    }
}

/// The path a file is stored at. A template file keeps its name under
/// `<team>/<board>/<name>`, so data retention does not sweep it; any other file goes to
/// its board's directory under today's directory under `boards/`.
fn destination_path(
    as_template: DuplicateAs,
    team_id: &str,
    board_id: &str,
    filename: &str,
) -> String {
    match as_template {
        DuplicateAs::Template => join3(team_id, board_id, filename),
        DuplicateAs::Copy => format!("{}/{board_id}/{filename}", base_file_path(now_millis())),
    }
}

/// The board a stored path belongs to. Every path this server stores a file at names its
/// board: `boards/<date>/<board>/<name>` for an upload, `<team>/<board>/<name>` for a
/// file of a template board and for a file without a record. Any other path belongs to
/// no board, in particular the original's `boards/<date>/<name>`.
fn board_of_path(path: &str) -> Option<&str> {
    let parts: Vec<&str> = path.split('/').collect();
    match parts.as_slice() {
        [UPLOADS_ROOT, _date, board, _name] => Some(board),
        [UPLOADS_ROOT, ..] => None,
        [_team, board, _name] => Some(board),
        _ => None,
    }
}

/// Returns whether the file stored at `path` belongs to the board. A file is given out,
/// copied and exported through its own board only, whatever layout it is stored in; a
/// file whose path names no board is given out through none.
pub(crate) fn belongs_to_board(path: &str, board_id: &str) -> bool {
    board_of_path(path) == Some(board_id)
}

/// The directory new uploads go to: `boards/YYYYMMDD` for today (UTC).
///
/// The original uses the server's local date; this port uses UTC, which only differs in
/// the directory name around midnight and never affects retrieval.
fn base_file_path(now_millis: i64) -> String {
    let (year, month, day) = civil_from_millis(now_millis);
    format!("{UPLOADS_ROOT}/{year:04}{month:02}{day:02}")
}

/// Joins three path segments with `/`.
fn join3(a: &str, b: &str, c: &str) -> String {
    format!("{a}/{b}/{c}")
}

/// Strips the one-character kind prefix from a stored name to get its record identifier
/// (`server/app/files.go:132`). An empty name has no identifier; the original panics,
/// this returns a server error.
fn file_info_id(name: &str) -> Result<String, AppError> {
    name.get(1..)
        .map(str::to_owned)
        .ok_or_else(|| AppError::internal("file name has no identifier"))
}

/// A fresh file-info record for a name, as `model.NewFileInfo` builds it.
fn new_file_info(name: &str) -> FileInfo {
    let now = now_millis();
    FileInfo {
        creator_id: "boards".to_owned(),
        create_at: now,
        update_at: now,
        name: name.to_owned(),
        extension: stored_extension(name),
        ..FileInfo::default()
    }
}

/// Converts a byte count to the signed size the record stores, saturating.
fn size_to_i64(size: u64) -> i64 {
    i64::try_from(size).unwrap_or(i64::MAX)
}

/// The year, month and day (UTC) of a time in milliseconds since the Unix epoch.
///
/// Howard Hinnant's `civil_from_days` algorithm.
fn civil_from_millis(millis: i64) -> (i64, u32, u32) {
    let days = millis.div_euclid(86_400_000);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = u32::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(1);
    let month = u32::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).unwrap_or(1);
    let year = if month <= 2 { year + 1 } else { year };
    (year, month, day)
}

/// Returns a storage error as a not-found when it is one, so callers can tell a missing
/// file from a real failure.
impl From<StorageError> for AppError {
    fn from(error: StorageError) -> Self {
        match error {
            StorageError::NotFound(_) => {
                tracing::debug!(%error, "not found in the file storage");
                Self::not_found(FILE)
            }
            other => Self::internal(other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_from_millis_matches_known_dates() {
        assert_eq!(civil_from_millis(0), (1970, 1, 1), "the Unix epoch");
        // 2026-11-10T05:15:39Z, a time observed on the oracle.
        assert_eq!(
            civil_from_millis(1_794_287_739_000),
            (2026, 11, 10),
            "a date in the files directory"
        );
    }

    #[test]
    fn stored_extension_takes_the_last_dot_lower_cased() {
        assert_eq!(stored_extension("a.tar.GZ"), ".gz", "the last extension");
        assert_eq!(
            stored_extension(".bashrc"),
            ".bashrc",
            "a dotfile is all ext"
        );
        assert_eq!(stored_extension("noext"), "", "no extension");
        assert_eq!(stored_extension("secret.JPEG"), ".jpeg", "lower cased");
    }

    #[test]
    fn upload_extension_normalises_jpeg() {
        assert_eq!(upload_extension(".jpeg"), ".jpg", "jpeg becomes jpg");
        assert_eq!(upload_extension(".png"), ".png", "others are kept");
    }

    #[test]
    fn base_name_keeps_only_the_last_segment() {
        assert_eq!(
            base_name("../../evil.sh"),
            "evil.sh",
            "traversal is stripped"
        );
        assert_eq!(base_name("plain.png"), "plain.png", "a plain name is kept");
        assert_eq!(
            base_name(r"c:\dir\x.png"),
            r"c:\dir\x.png",
            "backslashes stay"
        );
    }

    #[test]
    fn destination_path_lays_files_out_as_the_original() {
        // 2026-11-10T05:15:39Z.
        let now = 1_794_287_739_000;
        assert_eq!(
            base_file_path(now),
            "boards/20261110",
            "a normal upload goes to today's directory"
        );
        let copy = destination_path(DuplicateAs::Copy, "0", "board1", "7abc.png");
        assert!(
            copy.starts_with("boards/") && copy.ends_with("/7abc.png"),
            "a normal upload goes under boards/<date>/: {copy}"
        );
        assert_eq!(
            destination_path(DuplicateAs::Template, "0", "board1", "pic.png"),
            "0/board1/pic.png",
            "a template upload keeps its name under the board"
        );
    }

    #[test]
    fn a_stored_path_names_its_board_in_both_layouts_of_this_server() {
        for (path, board) in [
            ("boards/20261110/bboard/7file.png", Some("bboard")),
            ("0/bboard/logo.png", Some("bboard")),
            ("team-id/bboard/7file.png", Some("bboard")),
            // The original's upload layout names no board.
            ("boards/20230101/7file.png", None),
            ("boards/7file.png", None),
            ("boards/20261110/bboard/nested/7file.png", None),
            ("7file.png", None),
            ("0/bboard/../other/7file.png", None),
            ("", None),
        ] {
            assert_eq!(board_of_path(path), board, "path {path:?}");
        }
    }

    #[test]
    fn a_file_belongs_to_the_board_of_its_path_and_to_no_other() {
        assert!(
            belongs_to_board("boards/20261110/bboard/7file.png", "bboard"),
            "its own board"
        );
        assert!(
            !belongs_to_board("boards/20261110/bboard/7file.png", "bother"),
            "another board"
        );
        assert!(
            !belongs_to_board("boards/20230101/7file.png", "20230101"),
            "a path without a board belongs to none, whatever is asked"
        );
        assert!(
            !belongs_to_board("boards/20230101/7file.png", ""),
            "not to the empty board either"
        );
    }

    #[test]
    fn file_info_id_strips_the_kind_prefix() {
        assert_eq!(
            file_info_id("7abc").expect("a prefixed name has an id"),
            "abc",
            "the prefix is dropped"
        );
        assert!(file_info_id("").is_err(), "an empty name has no id");
    }
}
