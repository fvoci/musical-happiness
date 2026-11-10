//! Files attached to boards: storing uploads, finding them again and copying them when
//! cards are duplicated.
//!
//! A file is one board's. It is stored under a name the server makes up, a secret with
//! the upload's extension, and its record names the board, the stored name and the place
//! in the file storage. A file is given out, copied and exported only through the board
//! of its record: whoever may read one board cannot fetch another board's file by
//! knowing its name. A file without a record is given out through no board.

use std::collections::HashMap;

use fvoci_model::{
    Block, BlockPatch, BlockPatchBatch, BlockType, Board, Body, BodyChange, FileInfo, JsonMap,
    new_id,
};
use serde_json::Value;
use tokio::io::AsyncRead;

use super::App;
use crate::auth::now_millis;
use crate::auth::secret::new_secret;
use crate::error::AppError;
use crate::storage::StorageError;

/// What a request misses that asks for a file that is not there.
pub(crate) const FILE: &str = "file";

/// The directory uploads are stored under.
const UPLOADS_ROOT: &str = "boards";

impl App {
    /// Stores an uploaded file for a board and returns the name it is stored under,
    /// which is what blocks refer to it by.
    ///
    /// The stored name is a new secret followed by the lower-cased extension of
    /// `filename` (`.jpeg` becomes `.jpg`). Nothing of the client's name ends up in a
    /// path: it is kept in the record only.
    ///
    /// # Errors
    ///
    /// Returns a server error if the file or its record cannot be written.
    pub async fn save_file(
        &self,
        reader: &mut (dyn AsyncRead + Send + Unpin),
        board_id: &str,
        filename: &str,
    ) -> Result<String, AppError> {
        let base = base_name(filename);
        let extension = stored_extension(base);
        let stored_name = new_stored_name(&extension);
        let path = destination_path(board_id, &stored_name);

        let size = self.files().write(&path, reader).await.map_err(|error| {
            AppError::internal(format_args!(
                "unable to store the file in the files storage: {error}"
            ))
        })?;

        let info = FileInfo {
            id: new_id(),
            board_id: board_id.to_owned(),
            stored_name: stored_name.clone(),
            path,
            size: size_to_i64(size),
            name: base.to_owned(),
            extension,
            create_at: now_millis(),
        };
        self.store().save_file_info(&info).await?;
        Ok(stored_name)
    }

    /// Returns the record of the file a board has under a stored name, or `None` if the
    /// board has no such file: there is none of that name, or it is another board's.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the lookup fails.
    pub async fn file_info(
        &self,
        board_id: &str,
        stored_name: &str,
    ) -> Result<Option<FileInfo>, AppError> {
        if stored_name.is_empty() {
            return Ok(None);
        }
        match self.store().get_file_info(board_id, stored_name).await {
            Ok(info) => Ok(Some(info)),
            Err(error) if error.is_not_found() => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    /// Checks that every image of a body shows a file of the board (`docs/s6-design.md`
    /// 5.7).
    ///
    /// # Errors
    ///
    /// Returns a bad request that names `body` for an image of a file the board does not
    /// have, and the store's error if a lookup fails.
    pub async fn check_body_files(
        &self,
        board_id: &str,
        body: Option<&Body>,
    ) -> Result<(), AppError> {
        let Some(body) = body else {
            return Ok(());
        };
        for file_id in body.file_ids() {
            if self.file_info(board_id, file_id).await?.is_none() {
                return Err(AppError::bad_request(
                    "an image of the body shows no file of the board",
                )
                .with_field("body"));
            }
        }
        Ok(())
    }

    /// Opens a board's file for reading, or returns `None` if the board has no file of
    /// that stored name or the file is gone from the storage.
    ///
    /// # Errors
    ///
    /// Returns a server error if the storage or the record lookup fails.
    pub(crate) async fn open_stored_file(
        &self,
        board_id: &str,
        stored_name: &str,
    ) -> Result<Option<crate::storage::FileStream>, AppError> {
        let Some(info) = self.file_info(board_id, stored_name).await? else {
            return Ok(None);
        };
        if !self.files().exists(&info.path).await? {
            return Ok(None);
        }
        Ok(Some(self.files().read(&info.path, None).await?))
    }

    /// Copies the files that duplicated image and attachment blocks refer to and points
    /// each block at its copy.
    ///
    /// `board_id` is the board the blocks were copied from; `blocks` are the copies, as
    /// they are stored already, and are changed along with what is stored. A block whose
    /// file could not be copied ends up naming no file. For blocks that are not stored yet, see [`App::copy_card_files_into`].
    ///
    /// # Errors
    ///
    /// Returns the store's error if the updated blocks cannot be written. Copy failures
    /// are logged and do not fail the call.
    pub async fn copy_and_update_card_files(
        &self,
        board_id: &str,
        user_id: &str,
        blocks: &mut [Block],
    ) -> Result<(), AppError> {
        // A failure to copy does not stop the duplication: it is logged, and the copied
        // blocks end up naming no file.
        let new_file_names = match self.copy_card_files(board_id, blocks).await {
            Ok(names) => names,
            Err(error) => {
                tracing::error!(board_id, %error, "Could not copy files while duplicating board");
                HashMap::new()
            }
        };

        // Write the names of the copies to the stored blocks.
        let mut block_ids = Vec::new();
        let mut block_patches = Vec::new();
        for block in blocks.iter_mut() {
            if let Some(body) = block.body.as_mut()
                && !body.file_ids().is_empty()
            {
                rename_body_files(body, &new_file_names);
                block_ids.push(block.id.clone());
                block_patches.push(BlockPatch {
                    body: BodyChange::Set(body.clone()),
                    ..BlockPatch::default()
                });
                continue;
            }
            if block.block_type != BlockType::Image && block.block_type != BlockType::Attachment {
                continue;
            }
            let Some(Value::String(file_id)) = block.fields.get("fileId") else {
                continue;
            };
            let new_name = new_file_names.get(file_id).cloned().unwrap_or_default();
            // The caller's copies say what is stored: they are what is answered and
            // announced.
            block
                .fields
                .insert("fileId".to_owned(), Value::String(new_name.clone()));
            block.fields.remove("attachmentId");
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
                    AppError::from(error).context(format_args!(
                        "could not patch file IDs while duplicating board {board_id}"
                    ))
                })?;
            // The patch moved `updateAt` on: the caller's copies are what is answered and
            // announced, and a client edits them with that `updateAt` as `readAt`.
            for block in blocks.iter_mut() {
                if batch.block_ids.contains(&block.id) {
                    *block = self.store().get_block(&block.id).await?;
                }
            }
        }
        Ok(())
    }

    /// Copies the files that image and attachment blocks refer to, for blocks that are
    /// about to be stored as copies of blocks of `source_board_id`, and points each block
    /// at its copy. Nothing is written to the blocks' store: the caller stores them.
    ///
    /// A block whose file is not the source board's, or could not be copied, ends up
    /// naming no file. Failures are logged and do not fail the call.
    pub async fn copy_card_files_into(&self, source_board_id: &str, blocks: &mut [Block]) {
        let new_file_names = match self.copy_card_files(source_board_id, blocks).await {
            Ok(names) => names,
            Err(error) => {
                tracing::error!(source_board_id, %error, "could not copy the files of new blocks");
                HashMap::new()
            }
        };
        for block in blocks.iter_mut() {
            if let Some(body) = block.body.as_mut() {
                rename_body_files(body, &new_file_names);
            }
            if block.block_type != BlockType::Image && block.block_type != BlockType::Attachment {
                continue;
            }
            let Some(Value::String(file_id)) = block.fields.get("fileId") else {
                continue;
            };
            let new_name = new_file_names.get(file_id).cloned().unwrap_or_default();
            block
                .fields
                .insert("fileId".to_owned(), Value::String(new_name));
            block.fields.remove("attachmentId");
        }
    }

    /// Removes a file that was written but has no record, after the record could not be
    /// stored. A file that cannot be removed either is logged: nothing names it.
    async fn remove_stray_file(&self, path: &str) {
        if let Err(error) = self.files().delete(path).await {
            tracing::warn!(%error, path, "a file without a record could not be removed");
        }
    }

    /// Copies each file referred to by an image or attachment block and returns a map
    /// from the original stored names to those of the copies. Only files of the source
    /// board are copied.
    async fn copy_card_files(
        &self,
        source_board_id: &str,
        blocks: &[Block],
    ) -> Result<HashMap<String, String>, AppError> {
        let source_board = self.board(source_board_id).await?;

        let mut new_file_names = HashMap::new();
        let mut destination: Option<Board> = None;
        for (block, file_id) in blocks
            .iter()
            .flat_map(|block| files_of(block).into_iter().map(move |id| (block, id)))
        {
            // A file that two blocks show, or one body twice, is copied once.
            if new_file_names.contains_key(&file_id) {
                continue;
            }

            // Most copies stay on the source board; a destination is looked up once.
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

            // A block can name any file. One that is not the source board's is not
            // copied; the copied block ends up naming no file.
            let Some(source) = self.file_info(&source_board.id, &file_id).await? else {
                tracing::warn!(
                    source_board_id,
                    block_id = %block.id,
                    "not copying a file that the source board does not have"
                );
                continue;
            };

            let stored_name = new_stored_name(&source.extension);
            let copy = FileInfo {
                id: new_id(),
                board_id: destination_board.id.clone(),
                stored_name: stored_name.clone(),
                path: destination_path(&destination_board.id, &stored_name),
                create_at: now_millis(),
                ..source.clone()
            };
            // The bytes first, then the record that says where they are. A file that
            // could not be copied gets no record and no name: the block that pointed at
            // it ends up naming no file, not a file that is not there.
            if let Err(error) = self.files().copy(&source.path, &copy.path).await {
                tracing::error!(
                    %error,
                    source_path = source.path,
                    destination_path = copy.path,
                    "CopyCardFiles failed to copy file"
                );
                continue;
            }
            if let Err(error) = self.store().save_file_info(&copy).await {
                self.remove_stray_file(&copy.path).await;
                return Err(error.into());
            }
            new_file_names.insert(file_id, stored_name);
        }
        Ok(new_file_names)
    }
}

/// The stored names of the files a block shows: the file of an image or attachment
/// block, and the images of a card's body.
fn files_of(block: &Block) -> Vec<String> {
    if block.block_type == BlockType::Image || block.block_type == BlockType::Attachment {
        return file_reference(&block.fields).into_iter().collect();
    }
    block
        .body
        .as_ref()
        .map(|body| body.file_ids().into_iter().map(str::to_owned).collect())
        .unwrap_or_default()
}

/// Points the images of a copied body at the copies of their files. An image whose file
/// was not copied keeps the name it had: a body has no image without a file.
fn rename_body_files(body: &mut Body, new_file_names: &HashMap<String, String>) {
    body.rename_files(|old| new_file_names.get(old).cloned());
}

/// The file a block points at: its `fileId`, or failing that its `attachmentId`.
fn file_reference(fields: &JsonMap) -> Option<String> {
    for key in ["fileId", "attachmentId"] {
        if let Some(Value::String(value)) = fields.get(key) {
            return Some(value.clone());
        }
    }
    None
}

/// The last path segment of a client-supplied name, so that a name with directory parts
/// cannot escape its directory.
pub(crate) fn base_name(name: &str) -> &str {
    name.rsplit('/').next().unwrap_or(name)
}

/// The lower-cased extension of a name, with its leading dot, as kept in the record.
/// `.jpeg` is stored as `.jpg`.
fn stored_extension(name: &str) -> String {
    let extension = match name.rfind('.') {
        Some(index) => name[index..].to_lowercase(),
        None => String::new(),
    };
    // What ends up in a stored name and a path is plain: an extension with anything
    // else in it is dropped from the name. The record keeps the client's name.
    let plain = extension.len() <= MAX_EXTENSION_LENGTH
        && extension
            .bytes()
            .skip(1)
            .all(|byte| byte.is_ascii_alphanumeric());
    match extension.as_str() {
        _ if !plain => String::new(),
        ".jpeg" => ".jpg".to_owned(),
        _ => extension,
    }
}

/// The longest extension, dot included, that a stored name takes over from an upload.
const MAX_EXTENSION_LENGTH: usize = 17;

/// A new stored name: a secret, so that the name cannot be guessed, and the extension.
fn new_stored_name(extension: &str) -> String {
    format!("{}{extension}", new_secret())
}

/// Where a board's file is stored: `boards/YYYYMMDD/<board id>/<stored name>`, with the
/// date of the upload in UTC.
fn destination_path(board_id: &str, stored_name: &str) -> String {
    format!("{}/{board_id}/{stored_name}", base_file_path(now_millis()))
}

/// `boards/YYYYMMDD` for the UTC day of `now_millis`.
fn base_file_path(now_millis: i64) -> String {
    let (year, month, day) = civil_from_millis(now_millis);
    format!("{UPLOADS_ROOT}/{year:04}{month:02}{day:02}")
}

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
        assert_eq!(stored_extension("noext"), "", "no extension");
        assert_eq!(stored_extension("secret.JPEG"), ".jpg", "jpeg becomes jpg");
        assert_eq!(stored_extension("photo.png"), ".png", "others are kept");
    }

    #[test]
    fn an_extension_that_is_not_plain_stays_out_of_the_stored_name() {
        for name in [
            "x.p/ng",
            "x.png ",
            "x.a b",
            "x.%2e",
            "x.ä",
            "x.aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ] {
            assert_eq!(stored_extension(name), "", "name {name:?}");
        }
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
    fn a_file_is_stored_under_its_board_and_a_name_that_is_a_secret() {
        // 2026-11-10T05:15:39Z.
        let now = 1_794_287_739_000;
        assert_eq!(
            base_file_path(now),
            "boards/20261110",
            "an upload goes to the directory of its day"
        );

        let name = new_stored_name(".png");
        let (stem, extension) = name.split_at(crate::auth::secret::SECRET_LENGTH);
        assert_eq!(extension, ".png", "the extension follows the secret");
        assert!(!fvoci_model::is_id(stem), "the name is no identifier");
        assert_ne!(name, new_stored_name(".png"), "and differs every time");

        let path = destination_path("the-board", &name);
        assert!(
            path.starts_with("boards/") && path.ends_with(&format!("/the-board/{name}")),
            "the path names the board: {path}"
        );
    }
}
