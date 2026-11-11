//! Uploads in parts (`docs/s6-design.md` 5.6).
//!
//! A user opens a session for a file of a board, sends the file in numbered parts of a
//! size the server sets, and completes the session, which makes the file. Nothing of the
//! parts is a file before.
//!
//! With the local file storage, the parts are kept on the local disk under the temporary
//! directory, `uploads/<session>/`, each in a file of its own that the record of the
//! part names (`<number>.<random>`), until the session is completed or thrown away. With
//! S3, a session that has bytes to send is a multipart upload of the object store, made
//! at the file's final place when the session is opened: each part goes to the bucket as
//! it comes in, held in memory until it is checked, and the record of the part names the
//! object store's identifier of it (`docs/s6-design.md` 5.6, 6).

use std::io;
use std::path::{Path, PathBuf};

use bytes::{Bytes, BytesMut};
use futures_util::{Stream, StreamExt as _, TryStreamExt as _};
use fvoci_model::{Upload, UploadPart};
use fvoci_store::PartRecord;
use sha2::{Digest as _, Sha256};
use tokio::fs;
use tokio::io::AsyncWriteExt as _;
use tokio_util::io::{ReaderStream, StreamReader};

use super::App;
use super::files::FilePlace;
use crate::auth::now_millis;
use crate::auth::secret::new_secret;
use crate::error::{AppError, Reason};
use crate::storage::{S3Files, StorageError, WRITING_DIR};

/// How long a session lasts after it is opened (`docs/s6-design.md` 5.6, 7).
pub const UPLOAD_LIFETIME_MILLIS: i64 = 24 * 60 * 60 * 1000;

/// How many sessions one user may have open at once (`docs/s6-design.md` 5.6, 8).
pub const MAX_OPEN_UPLOADS: i64 = 8;

/// How long a closed session that is still there is left to whoever closed it before
/// the clean-up throws it away: a completion takes far less, and one that takes this
/// long has failed with the server.
pub const CLOSED_UPLOAD_GRACE_MILLIS: i64 = 60 * 60 * 1000;

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
    /// type that is too long; a 413 for a file larger than `maxfilesize`; a 429 if the
    /// user has [`MAX_OPEN_UPLOADS`] sessions open, or the open sessions would hold back
    /// more bytes than the user's share (twice the largest file) or the server's
    /// (`max_pending_upload_bytes`); and the store's error.
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
        let limit = self.largest_upload();
        if new.size > limit {
            return Err(AppError::payload_too_large(
                u64::try_from(limit).unwrap_or(u64::MAX),
            ));
        }

        // One server serves a database (`docs/configuration.md`): a lock in it is
        // enough to keep the check and the insertion together.
        let turn = self.opening_upload_turn().await;
        let now = now_millis();
        let usage = self.store().upload_usage(user_id, now).await?;
        if usage.user_sessions >= MAX_OPEN_UPLOADS {
            return Err(AppError::limited(
                format_args!("{MAX_OPEN_UPLOADS} uploads are open; complete or abandon one first"),
                Reason::TooManyUploads,
            ));
        }
        let user_share = limit.saturating_mul(2);
        if usage.user_bytes.saturating_add(new.size) > user_share {
            return Err(AppError::limited(
                format_args!(
                    "the open uploads would hold back more than {user_share} bytes; \
                     complete or abandon one first"
                ),
                Reason::UploadsPending,
            ));
        }
        let server_share =
            i64::try_from(self.config().max_pending_upload_bytes).unwrap_or(i64::MAX);
        if usage.server_bytes.saturating_add(new.size) > server_share {
            return Err(AppError::limited(
                "the server holds back as many bytes of uploads as it may; try again later",
                Reason::ServerUploadsPending,
            ));
        }
        let part_size = i64::try_from(self.config().upload_part_bytes).unwrap_or(i64::MAX);
        // An empty file has no part to send; its parts, none, are kept as on the disk.
        let multipart = self.files().multipart().filter(|_| new.size > 0);
        let path = multipart
            .map(|_| FilePlace::new(board_id, &new.name).path)
            .unwrap_or_default();
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
            path,
        };
        self.store().create_upload(&upload).await?;
        drop(turn);
        match multipart {
            Some(files) => self.start_multipart(upload, files).await,
            None => Ok(upload),
        }
    }

    /// Starts the multipart upload of a session in the object store and records it. Done
    /// once the session is stored, out of the turn of opening: the object store may be
    /// slow to answer, and the session holds its place in the limits meanwhile. A session
    /// that cannot be started is thrown away.
    async fn start_multipart(
        &self,
        mut upload: Upload,
        files: &S3Files,
    ) -> Result<Upload, AppError> {
        let multipart_id = match files.create_multipart(&upload.path).await {
            Ok(multipart_id) => multipart_id,
            Err(error) => {
                self.discard_upload(&upload).await;
                return Err(AppError::internal(format_args!(
                    "the upload could not be started in the files storage: {error}"
                )));
            }
        };
        upload.multipart_id = multipart_id;
        let recorded = self
            .store()
            .set_upload_multipart(&upload.id, &upload.multipart_id)
            .await;
        match recorded {
            Ok(true) => Ok(upload),
            Ok(false) => {
                self.discard_upload(&upload).await;
                Err(AppError::internal("the upload went away as it was started"))
            }
            Err(error) => {
                self.discard_upload(&upload).await;
                Err(error.into())
            }
        }
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
        if !upload.multipart_id.is_empty() {
            return self
                .put_multipart_part(upload, number, size, digest, body)
                .await;
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

    /// Receives a part of a multipart upload into memory, checks it, and sends it to the
    /// object store, where it replaces what was sent under its number.
    async fn put_multipart_part<E>(
        &self,
        upload: &Upload,
        number: i64,
        size: u64,
        digest: &PartDigest,
        body: impl Stream<Item = Result<Bytes, E>> + Unpin,
    ) -> Result<(), AppError>
    where
        E: std::fmt::Display,
    {
        let Some(files) = self.files().multipart() else {
            return Err(AppError::internal(
                "the upload is in an object store that is not configured",
            ));
        };
        let mut sink = PartSink::Memory(BytesMut::new());
        read_part(&mut sink, size, digest, body).await?;
        let PartSink::Memory(bytes) = sink else {
            unreachable!("the sink is the memory it was made as");
        };
        // Numbers start at 1, the object store's indexes at 0.
        let index = usize::try_from(number.saturating_sub(1)).unwrap_or_default();
        let part_id = match files
            .put_part(&upload.path, &upload.multipart_id, index, bytes.freeze())
            .await
        {
            Ok(part_id) => part_id,
            Err(error) => return Err(self.multipart_part_error(upload, &error).await),
        };
        let part = UploadPart {
            number,
            size: i64::try_from(size).unwrap_or(i64::MAX),
            part_id,
        };
        match self.store().put_upload_part(&upload.id, &part).await? {
            PartRecord::Recorded { .. } => Ok(()),
            // The part is in the object store all the same. Whoever closed the session
            // completes it with the parts recorded, or throws them all away.
            PartRecord::Closed => Err(closed()),
        }
    }

    /// What to answer for a part the object store did not take: a conflict if the
    /// session was closed or thrown away meanwhile, as the object store refuses parts of
    /// an upload that is completed or aborted; the server's failure otherwise.
    async fn multipart_part_error(&self, upload: &Upload, error: &StorageError) -> AppError {
        match self
            .store()
            .get_upload(&upload.id, &upload.user_id, now_millis())
            .await
        {
            Ok(current) if current.closed_at == 0 => AppError::internal(format_args!(
                "the part could not be stored in the files storage: {error}"
            )),
            Ok(_) => closed(),
            Err(lookup) if lookup.is_not_found() => closed(),
            Err(lookup) => lookup.into(),
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
        self.check_parts(upload).await?;
        if !self.store().close_upload(&upload.id, now_millis()).await? {
            return Err(closed());
        }
        // Read again once closed: a part recorded in between replaced one read above,
        // and none is recorded now.
        let parts = match self.check_parts(upload).await {
            Ok(parts) => parts,
            Err(error) => {
                self.discard_upload(upload).await;
                return Err(error);
            }
        };
        if !upload.multipart_id.is_empty() {
            return self.complete_multipart(upload, &parts).await;
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

    /// Completes the multipart upload of a closed session in the object store, which
    /// makes the file at its place, and records the file.
    async fn complete_multipart(
        &self,
        upload: &Upload,
        parts: &[UploadPart],
    ) -> Result<String, AppError> {
        let Some(files) = self.files().multipart() else {
            self.discard_upload(upload).await;
            return Err(AppError::internal(
                "the upload is in an object store that is not configured",
            ));
        };
        let part_ids = parts.iter().map(|part| part.part_id.clone()).collect();
        if let Err(error) = files
            .complete_multipart(&upload.path, &upload.multipart_id, part_ids)
            .await
        {
            self.discard_upload(upload).await;
            return Err(AppError::internal(format_args!(
                "the file could not be made from its parts: {error}"
            )));
        }
        // The upload is the file now: there is nothing left to abort.
        self.forget_upload(upload).await;
        let size = u64::try_from(upload.size).unwrap_or_default();
        let place = FilePlace::at(&upload.path, &upload.name);
        match self.record_file(&upload.board_id, place, size).await {
            Ok(stored_name) => Ok(stored_name),
            Err(error) => {
                // A file without a record is given out through no board; it is removed
                // rather than left in the bucket.
                if let Err(error) = self.files().delete(&upload.path).await {
                    tracing::warn!(%error, "an uploaded file without a record could not be removed");
                }
                Err(error)
            }
        }
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

    /// Throws a session away: its record, and the parts on the disk or in the object
    /// store. What is gone already is left gone; what cannot be removed is logged, and
    /// found again by the clean-up of the temporary directory, or by the bucket's rule
    /// for incomplete multipart uploads (`docs/fvoci-cdn.md`).
    pub async fn discard_upload(&self, upload: &Upload) {
        // Closed first: a part still coming in is refused rather than recorded.
        if let Err(error) = self.store().close_upload(&upload.id, now_millis()).await {
            tracing::warn!(%error, "an upload could not be closed before it was thrown away");
        }
        if !upload.multipart_id.is_empty()
            && let Some(files) = self.files().multipart()
            && let Err(error) = files
                .abort_multipart(&upload.path, &upload.multipart_id)
                .await
        {
            tracing::warn!(%error, "the parts of an upload could not be thrown away in the files storage");
        }
        self.forget_upload(upload).await;
    }

    /// Removes the record of a closed session and its directory of parts.
    async fn forget_upload(&self, upload: &Upload) {
        if let Err(error) = self.store().delete_upload(&upload.id).await {
            tracing::error!(%error, "the record of an upload could not be removed");
        }
        remove_dir_quietly(&self.upload_dir(&upload.id)).await;
    }

    /// The largest file a session takes: `maxfilesize`, which the configuration keeps
    /// positive and within what [`fvoci_model::MAX_UPLOAD_PARTS`] parts hold.
    pub fn largest_upload(&self) -> i64 {
        self.config().max_file_size
    }

    /// Throws away the sessions that have ended at `now`, and those closed for longer
    /// than [`CLOSED_UPLOAD_GRACE_MILLIS`] that are still there. Returns how many.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the sessions cannot be read.
    pub async fn clean_up_uploads(&self, now: i64) -> Result<usize, AppError> {
        let uploads = self
            .store()
            .uploads_to_clean_up(now, now.saturating_sub(CLOSED_UPLOAD_GRACE_MILLIS))
            .await?;
        for upload in &uploads {
            // Closed first: a part still coming in is refused rather than recorded.
            self.discard_upload(upload).await;
        }
        Ok(uploads.len())
    }

    /// Removes what the temporary directories hold that nothing will use: the parts of
    /// sessions that are gone, the directories of completions that were cut off, and the
    /// files the local storage was writing. Only for when no request is under way, as
    /// at the start, and only with one server to the files directory and the temporary
    /// directory (`docs/configuration.md`). Returns how many it removed.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the sessions cannot be read.
    pub async fn sweep_temporary_files(&self) -> Result<usize, AppError> {
        let sessions: std::collections::HashSet<String> =
            self.store().upload_ids().await?.into_iter().collect();
        let mut removed = 0;
        let uploads = self.config().tmp_dir().join("uploads");
        for (name, path) in entries(&uploads).await {
            // A session's directory is named by its identifier; anything else, such as a
            // directory a completion moved aside, is left over.
            if !sessions.contains(&name) && remove_entry(&path).await {
                removed += 1;
            }
        }
        if self.config().files_driver == "local" {
            let writing = Path::new(&self.config().files_path).join(WRITING_DIR);
            for (_, path) in entries(&writing).await {
                if remove_entry(&path).await {
                    removed += 1;
                }
            }
        }
        Ok(removed)
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
    body: impl Stream<Item = Result<Bytes, E>> + Unpin,
) -> Result<(), AppError>
where
    E: std::fmt::Display,
{
    let file = fs::File::create(path).await.map_err(|error| {
        // The directory went away under the request: the upload was closed meanwhile.
        if error.kind() == io::ErrorKind::NotFound {
            closed()
        } else {
            part_error(&error)
        }
    })?;
    let mut sink = PartSink::File(file);
    read_part(&mut sink, size, digest, body).await?;
    if let PartSink::File(file) = &mut sink {
        file.flush().await.map_err(|error| part_error(&error))?;
        file.sync_all().await.map_err(|error| part_error(&error))?;
    }
    Ok(())
}

/// Where a part is received into.
enum PartSink {
    /// A file of the local disk.
    File(fs::File),
    /// Memory, for a part that goes to the object store once it is checked.
    Memory(BytesMut),
}

impl PartSink {
    async fn write(&mut self, chunk: &[u8]) -> Result<(), AppError> {
        match self {
            Self::File(file) => file
                .write_all(chunk)
                .await
                .map_err(|error| part_error(&error)),
            Self::Memory(bytes) => {
                bytes.extend_from_slice(chunk);
                Ok(())
            }
        }
    }
}

/// Reads the body of a part into `sink`, checking its length and digest as it goes.
async fn read_part<E>(
    sink: &mut PartSink,
    size: u64,
    digest: &PartDigest,
    mut body: impl Stream<Item = Result<Bytes, E>> + Unpin,
) -> Result<(), AppError>
where
    E: std::fmt::Display,
{
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
        sink.write(&chunk).await?;
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

/// The entries of a directory with their names; none if it cannot be read.
async fn entries(directory: &Path) -> Vec<(String, PathBuf)> {
    let mut found = Vec::new();
    let mut listing = match fs::read_dir(directory).await {
        Ok(listing) => listing,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return found,
        Err(error) => {
            tracing::warn!(%error, "a temporary directory could not be read");
            return found;
        }
    };
    loop {
        match listing.next_entry().await {
            Ok(Some(entry)) => {
                found.push((
                    entry.file_name().to_string_lossy().into_owned(),
                    entry.path(),
                ));
            }
            Ok(None) => break,
            Err(error) => {
                tracing::warn!(%error, "a temporary directory could not be read to its end");
                break;
            }
        }
    }
    found
}

/// Removes a file or a directory that is left over, with what is in it. Returns whether
/// it was removed.
async fn remove_entry(path: &Path) -> bool {
    let removed = match fs::symlink_metadata(path).await {
        Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(path).await,
        Ok(_) => fs::remove_file(path).await,
        Err(error) => Err(error),
    };
    match removed {
        Ok(()) => true,
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(error) => {
            tracing::warn!(%error, "a left-over temporary file could not be removed");
            false
        }
    }
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
