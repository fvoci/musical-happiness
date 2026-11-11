//! Uploads in parts (`docs/s6-design.md` 5.6).
//!
//! A user opens a session for a file of a board, sends the file in numbered parts of a
//! size the server sets, and completes the session, which makes the file. The parts are
//! kept on the local disk under the temporary directory, `uploads/<session>/`, each in a
//! file of its own that the record of the part names (`<number>.<random>`), until the
//! session is completed or thrown away; nothing of them is a file before.

use std::io;
use std::path::{Path, PathBuf};

use bytes::Bytes;
use futures_util::{Stream, StreamExt as _, TryStreamExt as _};
use fvoci_model::{MAX_UPLOAD_PARTS, Upload, UploadPart};
use fvoci_store::PartRecord;
use sha2::{Digest as _, Sha256};
use tokio::fs;
use tokio::io::AsyncWriteExt as _;
use tokio_util::io::{ReaderStream, StreamReader};

use super::App;
use crate::auth::now_millis;
use crate::auth::secret::new_secret;
use crate::error::AppError;

/// How long a session lasts after it is opened (`docs/s6-design.md` 5.6, 7).
pub const UPLOAD_LIFETIME_MILLIS: i64 = 24 * 60 * 60 * 1000;

/// The longest file name a session takes, in characters.
pub const MAX_UPLOAD_NAME_CHARS: usize = 255;

/// What a refusal says a session is that is not there: none of that identifier, another
/// user's, or one that has ended.
const UPLOAD: &str = "upload";

/// A session to open: the file the client is about to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileToUpload {
    /// The name of the file.
    pub name: String,
    /// Its size in bytes.
    pub size: i64,
    /// Its content type as the client knows it, or empty.
    pub content_type: String,
}

/// The SHA-256 digest of a part, as the client sent it.
pub type PartDigest = [u8; 32];

impl App {
    /// Opens a session for `user_id` to upload a file to `board_id`. The caller has
    /// checked that the user may upload files to the board.
    ///
    /// # Errors
    ///
    /// Returns a bad request for an empty or too long name, a negative size or a content
    /// type that is too long; a 413 for a file larger than `maxfilesize`, or than
    /// [`MAX_UPLOAD_PARTS`] parts hold; and the store's error.
    pub async fn create_upload(
        &self,
        user_id: &str,
        board_id: &str,
        new: FileToUpload,
    ) -> Result<Upload, AppError> {
        if new.name.trim().is_empty() || new.name.chars().count() > MAX_UPLOAD_NAME_CHARS {
            return Err(AppError::bad_request(format_args!(
                "the name is 1 to {MAX_UPLOAD_NAME_CHARS} characters"
            ))
            .with_field("name"));
        }
        if new.content_type.chars().count() > MAX_UPLOAD_NAME_CHARS {
            return Err(AppError::bad_request(format_args!(
                "the content type is at most {MAX_UPLOAD_NAME_CHARS} characters"
            ))
            .with_field("contentType"));
        }
        if new.size < 0 {
            return Err(AppError::bad_request("the size is 0 or more bytes").with_field("size"));
        }
        let part_size = i64::try_from(self.config().upload_part_bytes).unwrap_or(i64::MAX);
        let max_file_size = self.config().max_file_size;
        let largest = part_size.saturating_mul(MAX_UPLOAD_PARTS);
        let limit = if max_file_size > 0 {
            max_file_size.min(largest)
        } else {
            largest
        };
        if new.size > limit {
            return Err(AppError::payload_too_large(
                u64::try_from(limit).unwrap_or(u64::MAX),
            ));
        }

        let now = now_millis();
        let upload = Upload {
            id: new_secret(),
            user_id: user_id.to_owned(),
            board_id: board_id.to_owned(),
            name: new.name,
            size: new.size,
            content_type: new.content_type,
            part_size,
            create_at: now,
            expires_at: now.saturating_add(UPLOAD_LIFETIME_MILLIS),
            closed_at: 0,
            multipart_id: String::new(),
        };
        self.store().create_upload(&upload).await?;
        Ok(upload)
    }

    /// Returns the user's session and the parts it has received.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if the user has no such session that is still going:
    /// none of that identifier, another user's, or one that has ended. And the store's
    /// error.
    pub async fn upload(
        &self,
        upload_id: &str,
        user_id: &str,
    ) -> Result<(Upload, Vec<UploadPart>), AppError> {
        let upload = match self
            .store()
            .get_upload(upload_id, user_id, now_millis())
            .await
        {
            Ok(upload) => upload,
            Err(error) if error.is_not_found() => return Err(AppError::not_found(UPLOAD)),
            Err(error) => return Err(error.into()),
        };
        let parts = self.store().get_upload_parts(&upload.id).await?;
        Ok((upload, parts))
    }

    /// Receives part `number` of an upload from `body`, whose length the request
    /// declared as `length`, and records it, replacing what was received under that
    /// number before. The part is checked as it is read: its length against the part's
    /// size, its bytes against `digest`. A part that fails a check is not kept.
    ///
    /// # Errors
    ///
    /// Returns a bad request for a number the upload has no part of, a declared length
    /// other than the part's size, a body that is longer or shorter than that, or one
    /// whose digest differs; a conflict if the session was closed meanwhile; and a
    /// server error if the part cannot be written.
    pub async fn put_upload_part<E>(
        &self,
        upload: &Upload,
        number: i64,
        length: u64,
        digest: &PartDigest,
        body: impl Stream<Item = Result<Bytes, E>> + Unpin,
    ) -> Result<(), AppError>
    where
        E: std::fmt::Display,
    {
        let Some(size) = upload.part_len(number) else {
            return Err(AppError::bad_request(format_args!(
                "the upload has parts 1 to {}",
                upload.part_count()
            ))
            .with_field("partNumber"));
        };
        let size = u64::try_from(size).unwrap_or_default();
        if length != size {
            return Err(AppError::bad_request(format_args!(
                "part {number} has {size} bytes; the request declares {length}"
            )));
        }
        if upload.closed_at != 0 {
            return Err(closed());
        }

        let directory = self.upload_dir(&upload.id);
        fs::create_dir_all(&directory)
            .await
            .map_err(|error| part_error(&error))?;
        // Every part that comes in gets a file of its own, which the record names: a part
        // sent again, or a request that is late, never touches a file that is recorded.
        let name = format!("{number}.{}", new_secret());
        let file = directory.join(&name);
        if let Err(error) = receive_part(&file, size, digest, body).await {
            remove_quietly(&file).await;
            return Err(error);
        }
        let record = self
            .store()
            .put_upload_part(
                &upload.id,
                &UploadPart {
                    number,
                    size: i64::try_from(size).unwrap_or(i64::MAX),
                    part_id: name,
                },
            )
            .await;
        match record {
            Ok(PartRecord::Recorded { replaced }) => {
                if let Some(replaced) = replaced.filter(|replaced| !replaced.is_empty()) {
                    remove_quietly(&directory.join(replaced)).await;
                }
                Ok(())
            }
            Ok(PartRecord::Closed) => {
                // The session was closed while the part came in. Whoever closed it owns
                // the directory, or has moved it away and this request made it again.
                remove_quietly(&file).await;
                if let Err(error) = fs::remove_dir(&directory).await {
                    // Not empty: what is left is the closer's, or another late part's.
                    tracing::debug!(%error, "the directory of a closed upload is left");
                }
                Err(closed())
            }
            Err(error) => {
                remove_quietly(&file).await;
                Err(error.into())
            }
        }
    }

    /// Completes an upload: checks that every part is there, makes the file of the
    /// board from the parts in order, and throws the session away. Returns the file's
    /// identifier. The caller has checked again that the user may upload files to the
    /// board.
    ///
    /// # Errors
    ///
    /// Returns a bad request naming the parts that are missing, with the session left
    /// as it is; a conflict if the session is being completed or thrown away already;
    /// and a server error if the file cannot be made, with the session thrown away.
    pub async fn complete_upload(&self, upload: &Upload) -> Result<String, AppError> {
        let parts = self.check_parts(upload).await?;
        if !self.store().close_upload(&upload.id, now_millis()).await? {
            return Err(closed());
        }
        // The parts are moved out of the way first: a part that is still coming in, or an
        // abandonment, finds no directory to change under the completion.
        let directory = self.upload_dir(&upload.id);
        let working = directory.with_extension(format!("completing-{}", new_secret()));
        // An empty file has no part, and so no directory yet.
        let moved = match fs::create_dir_all(&directory).await {
            Ok(()) => fs::rename(&directory, &working).await,
            Err(error) => Err(error),
        };
        let saved = match moved {
            Ok(()) => self.save_parts(upload, &parts, &working).await,
            Err(error) => Err(part_error(&error)),
        };
        self.discard_upload(upload).await;
        remove_dir_quietly(&working).await;
        saved
    }

    /// Checks that every part of an upload was received with its size, and returns the
    /// parts by number.
    async fn check_parts(&self, upload: &Upload) -> Result<Vec<UploadPart>, AppError> {
        let parts = self.store().get_upload_parts(&upload.id).await?;
        let missing: Vec<i64> = (1..=upload.part_count())
            .filter(|number| {
                !parts.iter().any(|part| {
                    part.number == *number && Some(part.size) == upload.part_len(*number)
                })
            })
            .collect();
        if !missing.is_empty() {
            let shown: Vec<String> = missing.iter().take(20).map(i64::to_string).collect();
            let more = if missing.len() > shown.len() {
                ", …"
            } else {
                ""
            };
            return Err(AppError::bad_request(format_args!(
                "parts missing: {}{more}",
                shown.join(", ")
            )));
        }
        // By number, as the store reads them, and none but these: a part of another
        // number is refused before it is recorded.
        Ok(parts)
    }

    /// Makes the file of an upload from its parts in `directory`, which must be the size
    /// they were received with.
    async fn save_parts(
        &self,
        upload: &Upload,
        parts: &[UploadPart],
        directory: &Path,
    ) -> Result<String, AppError> {
        let mut paths = Vec::with_capacity(parts.len());
        for part in parts {
            let path = directory.join(&part.part_id);
            let on_disk = fs::metadata(&path)
                .await
                .map_err(|error| part_error(&error))?
                .len();
            if Some(on_disk) != u64::try_from(part.size).ok() {
                return Err(AppError::internal(format_args!(
                    "part {} of an upload has {on_disk} bytes on the disk, not {}",
                    part.number, part.size
                )));
            }
            paths.push(path);
        }
        let bytes = futures_util::stream::iter(paths)
            .then(|path| async move { fs::File::open(path).await })
            .map_ok(ReaderStream::new)
            .try_flatten();
        let mut reader = StreamReader::new(Box::pin(bytes));
        self.save_file(&mut reader, &upload.board_id, &upload.name)
            .await
    }

    /// Abandons a session at the request of its user, and throws it away.
    ///
    /// # Errors
    ///
    /// Returns a conflict if the session is being completed or thrown away already, and
    /// the store's error.
    pub async fn abandon_upload(&self, upload: &Upload) -> Result<(), AppError> {
        if !self.store().close_upload(&upload.id, now_millis()).await? {
            return Err(closed());
        }
        self.discard_upload(upload).await;
        Ok(())
    }

    /// Throws a session away: its record, and the parts on the disk. What is gone
    /// already is left gone; what cannot be removed is logged, and found again by the
    /// clean-up of the temporary directory.
    pub async fn discard_upload(&self, upload: &Upload) {
        if let Err(error) = self.store().close_upload(&upload.id, now_millis()).await {
            tracing::warn!(%error, "an upload could not be closed before it was thrown away");
        }
        if let Err(error) = self.store().delete_upload(&upload.id).await {
            tracing::error!(%error, "the record of an upload could not be removed");
        }
        remove_dir_quietly(&self.upload_dir(&upload.id)).await;
    }

    /// The directory that holds the parts of an upload.
    pub(crate) fn upload_dir(&self, upload_id: &str) -> PathBuf {
        self.config().tmp_dir().join("uploads").join(upload_id)
    }
}

/// Writes the body to `path`, checking its length and digest as it goes.
async fn receive_part<E>(
    path: &Path,
    size: u64,
    digest: &PartDigest,
    mut body: impl Stream<Item = Result<Bytes, E>> + Unpin,
) -> Result<(), AppError>
where
    E: std::fmt::Display,
{
    let mut file = fs::File::create(path).await.map_err(|error| {
        // The directory went away under the request: the upload was closed meanwhile.
        if error.kind() == io::ErrorKind::NotFound {
            closed()
        } else {
            part_error(&error)
        }
    })?;
    let mut hasher = Sha256::new();
    let mut received: u64 = 0;
    while let Some(chunk) = body.next().await {
        let chunk = chunk.map_err(|error| {
            AppError::bad_request(format_args!("the part could not be read: {error}"))
        })?;
        received = received.saturating_add(u64::try_from(chunk.len()).unwrap_or(u64::MAX));
        if received > size {
            return Err(AppError::bad_request(format_args!(
                "the part is longer than its {size} bytes"
            )));
        }
        hasher.update(&chunk);
        file.write_all(&chunk)
            .await
            .map_err(|error| part_error(&error))?;
    }
    if received != size {
        return Err(AppError::bad_request(format_args!(
            "the part has {received} of its {size} bytes"
        )));
    }
    if hasher.finalize().as_slice() != digest {
        return Err(AppError::bad_request(
            "the part's SHA-256 digest is not the one of the Content-Digest header",
        )
        .with_field("Content-Digest"));
    }
    file.flush().await.map_err(|error| part_error(&error))?;
    file.sync_all().await.map_err(|error| part_error(&error))?;
    Ok(())
}

/// The refusal of a part or a completion for a session that is closed.
fn closed() -> AppError {
    AppError::conflict("the upload is being completed or thrown away")
}

/// A part that cannot be written: the server's failure.
fn part_error(error: &io::Error) -> AppError {
    AppError::internal(format_args!("the part could not be stored: {error}"))
}

/// Removes a directory of parts that is not to be kept, if it is there.
async fn remove_dir_quietly(directory: &Path) {
    match fs::remove_dir_all(directory).await {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => tracing::error!(%error, "the parts of an upload could not be removed"),
    }
}

/// Removes a file that is not to be kept, if it is there.
async fn remove_quietly(path: &Path) {
    match fs::remove_file(path).await {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => tracing::warn!(%error, "a part that is not kept could not be removed"),
    }
}
