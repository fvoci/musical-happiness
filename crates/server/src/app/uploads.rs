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
//! at the file's final place when the session is opened: each part is received into a
//! file of that directory and checked as on the local disk, then sent to the bucket and
//! the file removed, and the record of the part names the object store's identifier of
//! it (`docs/s6-design.md` 5.6, 6).

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use futures_util::{Stream, StreamExt as _, TryStreamExt as _};
use fvoci_model::{Upload, UploadPart, UploadPurpose};
use fvoci_store::PartRecord;
use sha2::{Digest as _, Sha256};
use tokio::fs;
use tokio::io::AsyncWriteExt as _;
use tokio::sync::{OwnedMutexGuard, Semaphore};
use tokio_util::io::{ReaderStream, StreamReader};

use super::App;
use super::files::FilePlace;
use crate::auth::now_millis;
use crate::auth::secret::new_secret;
use crate::error::{AppError, Reason};
use crate::storage::{S3Files, WRITING_DIR};

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

/// How many parts of the configured size the uploads to the object store hold in memory
/// at once, while each is read from its file and sent to the bucket. A part that would
/// take more waits on the disk until the memory is there.
pub const PARTS_IN_MEMORY: u64 = 8;

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

/// The memory of [`PARTS_IN_MEMORY`] parts of `part_bytes`, in KiB.
/// What a clean-up of the upload sessions did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CleanedUp {
    /// The sessions thrown away.
    pub removed: usize,
    /// The sessions that could not wholly be thrown away.
    pub left: usize,
}

pub(crate) fn part_memory_kib(part_bytes: u64) -> usize {
    let kib = part_bytes.div_ceil(1024).saturating_mul(PARTS_IN_MEMORY);
    usize::try_from(kib)
        .unwrap_or(usize::MAX)
        .min(Semaphore::MAX_PERMITS)
}

/// A turn for each session. A part of an upload to the object store is checked against
/// its session, sent to the bucket and recorded in the session's turn, and the session is
/// closed and completed, abandoned or thrown away in it: what the bucket holds under a
/// number is the part recorded under it, and nothing reaches the bucket once the session
/// is closed. The object store keeps one part to a number, where the local disk keeps a
/// file to each part that comes in. Kept in the server's memory: one server serves a
/// database (`docs/configuration.md`). A turn is there while someone holds or waits for
/// it, and no longer.
#[derive(Debug, Default)]
pub(crate) struct UploadTurns(Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>);

impl UploadTurns {
    /// Waits for the turn of a session.
    async fn take(&self, upload_id: &str) -> Turn<'_> {
        let turn = {
            let mut turns = self.0.lock().expect("the upload turns are not poisoned");
            Arc::clone(turns.entry(upload_id.to_owned()).or_default())
        };
        Turn {
            turns: self,
            upload_id: upload_id.to_owned(),
            guard: Some(turn.lock_owned().await),
        }
    }
}

/// The turn of a session, which is given up when this is dropped. The guard is an
/// `Option` only so that the drop can look at the count while it still holds it.
#[derive(Debug)]
struct Turn<'turns> {
    turns: &'turns UploadTurns,
    upload_id: String,
    guard: Option<OwnedMutexGuard<()>>,
}

impl Drop for Turn<'_> {
    fn drop(&mut self) {
        let Some(guard) = self.guard.take() else {
            return;
        };
        let mut turns = self
            .turns
            .0
            .lock()
            .expect("the upload turns are not poisoned");
        // The map's and this one: nobody else holds or waits for the turn, and nobody
        // can start to while the map is locked.
        if Arc::strong_count(OwnedMutexGuard::mutex(&guard)) == 2 {
            turns.remove(&self.upload_id);
        }
        drop(guard);
    }
}

impl App {
    /// Opens a session for `user_id` to upload a file to `board_id`. The caller has
    /// checked that the user may upload files to the board.
    ///
    /// # Errors
    ///
    /// Returns a bad request for an empty or too long name, a negative size or a content
    /// type that is too long; a 413 for a file larger than `maxfilesize`; a 429 if the
    /// user has [`MAX_OPEN_UPLOADS`] sessions open, or the open sessions would hold back
    /// more bytes than the user's share (twice the largest upload, a file or an archive)
    /// or the server's (`max_pending_upload_bytes`); and the store's error.
    pub async fn create_upload(
        &self,
        user_id: &str,
        board_id: &str,
        new: FileToUpload,
    ) -> Result<Upload, AppError> {
        self.open_upload(user_id, UploadPurpose::File, board_id, new)
            .await
    }

    /// Opens a session for `user_id` to upload an archive to import into `team_id`
    /// (`docs/s6-design.md` 5.6, 9). Its parts are kept on the local disk whatever the
    /// file storage: the import reads the archive at random. The caller has checked that
    /// the user may import into the team.
    ///
    /// # Errors
    ///
    /// Those of [`App::create_upload`], with a 413 for an archive larger than
    /// `max_import_bytes`.
    pub async fn create_import_upload(
        &self,
        user_id: &str,
        team_id: &str,
        new: FileToUpload,
    ) -> Result<Upload, AppError> {
        self.open_upload(user_id, UploadPurpose::Import, team_id, new)
            .await
    }

    /// Opens a session for a purpose, to a board (a file) or a team (an import).
    async fn open_upload(
        &self,
        user_id: &str,
        purpose: UploadPurpose,
        target: &str,
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
        let limit = match purpose {
            UploadPurpose::File => self.largest_upload(),
            UploadPurpose::Import => self.largest_import(),
        };
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
        let user_share = self
            .largest_upload()
            .max(self.largest_import())
            .saturating_mul(2);
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
        // An archive's parts are on the disk too.
        let multipart = self
            .files()
            .multipart()
            .filter(|_| new.size > 0 && purpose == UploadPurpose::File);
        let path = multipart
            .map(|_| FilePlace::new(target, &new.name).path)
            .unwrap_or_default();
        let (board_id, team_id) = match purpose {
            UploadPurpose::File => (target.to_owned(), String::new()),
            UploadPurpose::Import => (String::new(), target.to_owned()),
        };
        let upload = Upload {
            id: new_secret(),
            user_id: user_id.to_owned(),
            purpose,
            board_id,
            team_id,
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
        if !upload.multipart_id.is_empty() {
            // The part was received at the client's pace on the disk; the memory and the
            // turn are held only while it goes to the bucket.
            // The directory stays for the other parts that come in; it goes with the
            // session.
            let sent = self.send_multipart_part(upload, number, size, &file).await;
            remove_quietly(&file).await;
            return sent;
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

    /// Sends a part of a multipart upload, received and checked in `file`, to the object
    /// store, where it replaces what was sent under its number, and records it.
    async fn send_multipart_part(
        &self,
        upload: &Upload,
        number: i64,
        size: u64,
        file: &Path,
    ) -> Result<(), AppError> {
        let Some(files) = self.files().multipart() else {
            return Err(AppError::internal(
                "the upload is in an object store that is not configured",
            ));
        };
        // The turn first: a part waiting for it holds no memory.
        let _turn = self.upload_turns().take(&upload.id).await;
        // Checked in the turn: a session closed meanwhile takes nothing more.
        self.check_open(upload).await?;
        let kib = u32::try_from(size.div_ceil(1024))
            .expect("a part is at most 5 GiB, checked when the server starts");
        let _memory = Arc::clone(self.part_memory())
            .acquire_many_owned(kib)
            .await
            .map_err(|_| AppError::internal("the memory for parts is closed"))?;
        let bytes = fs::read(file).await.map_err(|error| {
            // The session's directory went with it while the part came in.
            if error.kind() == io::ErrorKind::NotFound {
                closed()
            } else {
                part_error(&error)
            }
        })?;
        let bytes = Bytes::from(bytes);
        // Numbers start at 1, the object store's indexes at 0.
        let index = usize::try_from(number.saturating_sub(1)).unwrap_or_default();
        let part_id = match files
            .put_part(&upload.path, &upload.multipart_id, index, vec![bytes])
            .await
        {
            Ok(part_id) => part_id,
            Err(error) => {
                // The bucket may have taken it with its answer lost.
                self.forget_part(upload, number).await;
                return Err(AppError::internal(format_args!(
                    "the part could not be stored in the files storage: {error}"
                )));
            }
        };
        let part = UploadPart {
            number,
            size: i64::try_from(size).unwrap_or(i64::MAX),
            part_id,
        };
        match self.store().put_upload_part(&upload.id, &part).await {
            Ok(PartRecord::Recorded { .. }) => Ok(()),
            // Not in the turn, which closes it in its turn; left as an answer all the same.
            Ok(PartRecord::Closed) => Err(closed()),
            Err(error) => {
                self.forget_part(upload, number).await;
                Err(error.into())
            }
        }
    }

    /// Forgets the record of a part when the bucket may hold another part under its
    /// number, so that the number is sent again.
    async fn forget_part(&self, upload: &Upload, number: i64) {
        if let Err(error) = self.store().delete_upload_part(&upload.id, number).await {
            tracing::error!(%error, "the record of a part the bucket may have replaced could not be removed");
        }
    }

    /// Checks that a session is still open: a conflict if it was closed or has gone, as by
    /// a completion, an abandonment or the clean-up.
    async fn check_open(&self, upload: &Upload) -> Result<(), AppError> {
        match self
            .store()
            .get_upload(&upload.id, &upload.user_id, now_millis())
            .await
        {
            Ok(current) if current.closed_at == 0 => Ok(()),
            Ok(_) => Err(closed()),
            Err(error) if error.is_not_found() => Err(closed()),
            Err(error) => Err(error.into()),
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
        if upload.purpose != UploadPurpose::File {
            return Err(AppError::bad_request(
                "the upload is of an archive; import it with importArchive",
            )
            .with_field("uploadId"));
        }
        // Before the session is closed, so that a missing part leaves it open.
        self.check_parts(upload).await?;
        let _turn = self.upload_turns().take(&upload.id).await;
        if !self.store().close_upload(&upload.id, now_millis()).await? {
            return Err(closed());
        }
        // Read again once closed: a part recorded in between replaced one read above,
        // and none is recorded now.
        let parts = match self.check_parts(upload).await {
            Ok(parts) => parts,
            Err(error) => {
                self.discard_in_turn(upload).await;
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
        self.discard_in_turn(upload).await;
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
            self.discard_in_turn(upload).await;
            return Err(AppError::internal(
                "the upload is in an object store that is not configured",
            ));
        };
        let part_ids = parts.iter().map(|part| part.part_id.clone()).collect();
        if let Err(error) = files
            .complete_multipart(&upload.path, &upload.multipart_id, part_ids)
            .await
        {
            // The completion may have taken effect with its answer lost: the object, which
            // has no record, goes with the session.
            self.discard_in_turn(upload).await;
            return Err(AppError::internal(format_args!(
                "the file could not be made from its parts: {error}"
            )));
        }
        // Recorded before the session is forgotten: a server that stops in between leaves
        // a closed session, which the clean-up throws away with the object if it has no
        // record.
        let size = u64::try_from(upload.size).unwrap_or_default();
        let place = FilePlace::at(&upload.path, &upload.name);
        let recorded = self.record_file(&upload.board_id, place, size).await;
        // A record whose answer was lost may be there all the same.
        if recorded.is_err() && !self.remove_if_unrecorded(upload).await {
            // Left closed for the clean-up to look at again.
            return recorded;
        }
        // The upload is the file now, or gone: there is nothing left to abort.
        self.forget_upload(upload).await;
        recorded
    }

    /// Removes a file of the object store that has no record: one is given out through no
    /// board, and only taking room.
    async fn remove_unrecorded(&self, path: &str) {
        if let Err(error) = self.files().delete(path).await {
            tracing::warn!(%error, "an uploaded file without a record could not be removed");
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
        let mut reader = parts_reader(parts, directory).await?;
        self.save_file(&mut reader, &upload.board_id, &upload.name)
            .await
    }

    /// Imports the archive of an upload into its team, as its user, and throws the
    /// upload away: checks that every part is there, joins them into one file on the
    /// local disk, and imports from that. The caller has checked again that the user may
    /// import into the team.
    ///
    /// # Errors
    ///
    /// Returns a bad request for an upload of a file, or one that misses parts (left as
    /// it is); a conflict if it is being completed or thrown away already; and the
    /// errors of the import, with the upload thrown away.
    pub async fn import_upload(&self, upload: &Upload) -> Result<(), AppError> {
        if upload.purpose != UploadPurpose::Import {
            return Err(AppError::bad_request(
                "the upload is of a file; complete it with completeUpload",
            )
            .with_field("uploadId"));
        }
        // Before the session is closed, so that a missing part leaves it open.
        self.check_parts(upload).await?;
        let turn = self.upload_turns().take(&upload.id).await;
        if !self.store().close_upload(&upload.id, now_millis()).await? {
            return Err(closed());
        }
        let parts = match self.check_parts(upload).await {
            Ok(parts) => parts,
            Err(error) => {
                self.discard_in_turn(upload).await;
                return Err(error);
            }
        };
        // Moved aside, as for a completion: the session's directory goes with it, the
        // archive stays until it is imported. A server that stops meanwhile leaves a
        // directory that is no session's, which the start-up sweep removes.
        let directory = self.upload_dir(&upload.id);
        let working = directory.with_extension(format!("importing-{}", new_secret()));
        let moved = match fs::create_dir_all(&directory).await {
            Ok(()) => fs::rename(&directory, &working).await,
            Err(error) => Err(error),
        };
        let archive = working.join("archive.boardarchive");
        let joined = match moved {
            Ok(()) => join_parts(&parts, &working, &archive).await,
            Err(error) => Err(part_error(&error)),
        };
        self.discard_in_turn(upload).await;
        drop(turn);
        let imported = match joined {
            Ok(()) => {
                self.import_team_archive(&archive, &upload.team_id, &upload.user_id)
                    .await
            }
            Err(error) => Err(error),
        };
        remove_dir_quietly(&working).await;
        imported
    }

    /// Abandons a session at the request of its user, and throws it away.
    ///
    /// # Errors
    ///
    /// Returns a conflict if the session is being completed or thrown away already, and
    /// the store's error.
    pub async fn abandon_upload(&self, upload: &Upload) -> Result<(), AppError> {
        let _turn = self.upload_turns().take(&upload.id).await;
        if !self.store().close_upload(&upload.id, now_millis()).await? {
            return Err(closed());
        }
        self.discard_in_turn(upload).await;
        Ok(())
    }

    /// Throws a session away: its record, and the parts on the disk or in the object
    /// store. What is gone already is left gone; what cannot be removed is logged, and
    /// found again by the clean-up of the temporary directory, or by the bucket's rule
    /// for incomplete multipart uploads (`docs/fvoci-cdn.md`). Returns whether all of it
    /// went: false if something was logged.
    pub async fn discard_upload(&self, upload: &Upload) -> bool {
        let _turn = self.upload_turns().take(&upload.id).await;
        self.discard_in_turn(upload).await
    }

    /// [`App::discard_upload`] in the session's turn, which the caller holds.
    async fn discard_in_turn(&self, upload: &Upload) -> bool {
        let mut settled = true;
        // Closed first: a part still coming in is refused rather than recorded.
        if let Err(error) = self.store().close_upload(&upload.id, now_millis()).await {
            tracing::warn!(%error, "an upload could not be closed before it was thrown away");
            settled = false;
        }
        if !upload.multipart_id.is_empty()
            && let Some(files) = self.files().multipart()
            && let Err(error) = files
                .abort_multipart(&upload.path, &upload.multipart_id)
                .await
        {
            tracing::warn!(%error, "the parts of an upload could not be thrown away in the files storage");
            settled = false;
        }
        if !upload.multipart_id.is_empty() && !self.remove_if_unrecorded(upload).await {
            // Left closed for the clean-up to look at again, once its grace is over.
            return false;
        }
        self.forget_upload(upload).await && settled
    }

    /// Removes the object at the place of a multipart upload unless a file's record names
    /// it: a completion that took effect without its file being recorded, as when the
    /// answer was lost or the server stopped in between, leaves an object nothing gives
    /// out. The place is the session's own, a new secret. Returns whether it is settled:
    /// false if whether the record is there could not be read.
    async fn remove_if_unrecorded(&self, upload: &Upload) -> bool {
        let stored_name = upload.path.rsplit('/').next().unwrap_or_default();
        match self.file_info(&upload.board_id, stored_name).await {
            Ok(None) => {
                self.remove_unrecorded(&upload.path).await;
                true
            }
            Ok(Some(_)) => true,
            Err(error) => {
                tracing::warn!(%error, "whether an uploaded file has a record could not be read");
                false
            }
        }
    }

    /// Removes the record of a closed session and its directory of parts. Returns
    /// whether the record went.
    async fn forget_upload(&self, upload: &Upload) -> bool {
        let forgotten = match self.store().delete_upload(&upload.id).await {
            Ok(()) => true,
            Err(error) => {
                tracing::error!(%error, "the record of an upload could not be removed");
                false
            }
        };
        remove_dir_quietly(&self.upload_dir(&upload.id)).await;
        forgotten
    }

    /// The largest file a session takes: `maxfilesize`, which the configuration keeps
    /// positive and within what [`fvoci_model::MAX_UPLOAD_PARTS`] parts hold.
    pub fn largest_upload(&self) -> i64 {
        self.config().max_file_size
    }

    /// The largest archive an import takes: `max_import_bytes`, which the configuration
    /// keeps positive and within what [`fvoci_model::MAX_UPLOAD_PARTS`] parts hold.
    pub fn largest_import(&self) -> i64 {
        i64::try_from(self.config().max_import_bytes).unwrap_or(i64::MAX)
    }

    /// Throws away the sessions that have ended at `now`, and those closed for longer
    /// than [`CLOSED_UPLOAD_GRACE_MILLIS`] that are still there. Returns how many went,
    /// and how many could not wholly go (logged; the next clean-up finds them again).
    ///
    /// # Errors
    ///
    /// Returns the store's error if the sessions cannot be read.
    pub async fn clean_up_uploads(&self, now: i64) -> Result<CleanedUp, AppError> {
        let uploads = self
            .store()
            .uploads_to_clean_up(now, now.saturating_sub(CLOSED_UPLOAD_GRACE_MILLIS))
            .await?;
        let mut cleaned = CleanedUp::default();
        for upload in &uploads {
            if self.discard_upload(upload).await {
                cleaned.removed += 1;
            } else {
                cleaned.left += 1;
            }
        }
        Ok(cleaned)
    }

    /// Removes what the temporary directories hold that nothing will use: the parts of
    /// sessions that are gone, the directories of completions that were cut off, the
    /// archives being exported, and the files the local storage was writing. Only for
    /// when no request is under way, as at the start, and only with one server to the
    /// files directory and the temporary directory (`docs/configuration.md`). Returns
    /// how many it removed.
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
        // Archives being written when the server stopped.
        let exports = self.config().tmp_dir().join(super::export::EXPORTS_DIR);
        for (_, path) in entries(&exports).await {
            if remove_entry(&path).await {
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

/// The parts in `directory` read one after the other, after checking that each is the
/// size it was received with.
async fn parts_reader(
    parts: &[UploadPart],
    directory: &Path,
) -> Result<impl tokio::io::AsyncRead + Send + Unpin + use<>, AppError> {
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
    Ok(StreamReader::new(Box::pin(bytes)))
}

/// Joins the parts in `directory` into the file `to`, in order, removing each once it is
/// in: the disk holds the archive and one part at most, not the archive twice.
async fn join_parts(parts: &[UploadPart], directory: &Path, to: &Path) -> Result<(), AppError> {
    let mut file = fs::File::create(to)
        .await
        .map_err(|error| part_error(&error))?;
    for part in parts {
        let path = directory.join(&part.part_id);
        let mut reader = fs::File::open(&path)
            .await
            .map_err(|error| part_error(&error))?;
        let copied = tokio::io::copy(&mut reader, &mut file)
            .await
            .map_err(|error| part_error(&error))?;
        if Some(copied) != u64::try_from(part.size).ok() {
            return Err(AppError::internal(format_args!(
                "part {} of an upload has {copied} bytes on the disk, not {}",
                part.number, part.size
            )));
        }
        drop(reader);
        remove_quietly(&path).await;
    }
    file.flush().await.map_err(|error| part_error(&error))?;
    Ok(())
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
