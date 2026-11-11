//! Exporting boards to the archive format of version 3.
//!
//! An archive is a ZIP holding `version.json` and, per board, a directory with a
//! `board.jsonl` and the board's files. Each line of `board.jsonl` is a board, a block
//! or a member as the contract has them (`contracts/openapi.json`), bodies included; the
//! files are those the blocks show, the images of bodies among them
//! (`docs/s6-design.md` 3.10). The container is the original's
//! (`server/app/export.go`); version 2 was its own.

use std::collections::HashSet;
use std::io::{BufWriter, Write as _};
use std::path::Path;

use futures_util::StreamExt as _;
use fvoci_model::{ArchiveHeader, ArchiveLine, Board};
use serde::Serialize;
use tokio::fs;
use zip::write::{SimpleFileOptions, ZipWriter};

use super::App;
use super::doomed::Doomed;
use crate::auth::now_millis;
use crate::auth::secret::new_secret;
use crate::error::AppError;

/// The archive format version this server writes.
pub(super) const ARCHIVE_VERSION: i64 = 3;

/// The directory of the temporary directory exports are written in.
pub(crate) const EXPORTS_DIR: &str = "exports";

/// How much of an entry is gathered before it is handed to the archive writer.
const WRITE_CHUNK_BYTES: usize = 1024 * 1024;

/// An archive written to a temporary file, ready to be sent: the file is open, and gone
/// from its directory.
#[derive(Debug)]
pub struct ExportedArchive {
    /// The archive, read from its start.
    pub file: fs::File,
    /// Its size in bytes.
    pub size: u64,
}

impl App {
    /// Builds an archive of the given boards in a temporary file
    /// (`docs/s6-design.md` 4.3) and returns it open.
    ///
    /// Each board is written with its blocks and members; a file a block shows is added
    /// if it exists and left as an empty entry if it is missing, as the original does.
    /// The file is removed from its directory once it is open, so that nothing of it is
    /// left however the sending ends; what a stopped server leaves is removed when it
    /// starts again.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if a board does not exist, or a server error if the
    /// archive cannot be written.
    pub async fn export_archive(
        &self,
        team_id: &str,
        board_ids: &[String],
    ) -> Result<ExportedArchive, AppError> {
        let mut boards = Vec::with_capacity(board_ids.len());
        for id in board_ids {
            boards.push(self.store().get_board(id).await.map_err(|error| {
                AppError::from(error).context(format_args!("could not fetch board {id}"))
            })?);
        }

        let directory = self.config().tmp_dir().join(EXPORTS_DIR);
        fs::create_dir_all(&directory)
            .await
            .map_err(|error| io_error(&error))?;
        // Removed however the export ends, also when its request is dropped meanwhile;
        // what is sent is read from the file opened before.
        let path = Doomed::file(directory.join(format!("{}.boardarchive", new_secret())));
        self.write_archive(path.path(), team_id, &boards).await?;
        let file = fs::File::open(path.path())
            .await
            .map_err(|error| io_error(&error))?;
        path.remove().await;
        let size = file
            .metadata()
            .await
            .map_err(|error| io_error(&error))?
            .len();
        Ok(ExportedArchive { file, size })
    }

    /// Writes the archive of `boards` to a new file at `path`.
    async fn write_archive(
        &self,
        path: &Path,
        team_id: &str,
        boards: &[Board],
    ) -> Result<(), AppError> {
        let file = fs::File::create(path)
            .await
            .map_err(|error| io_error(&error))?
            .into_std()
            .await;
        let mut writer = ArchiveWriter::new(file);
        writer.start_entry("version.json").await?;
        let header = ArchiveHeader {
            version: ARCHIVE_VERSION,
            date: now_millis(),
        };
        let bytes = serde_json::to_vec(&header).map_err(|error| {
            AppError::internal(format_args!("cannot write archive header: {error}"))
        })?;
        writer.write(&bytes).await?;
        for board in boards {
            self.write_board(&mut writer, board, team_id).await?;
        }
        writer.finish().await
    }

    /// Writes one board's directory: its `board.jsonl` and its files.
    async fn write_board(
        &self,
        writer: &mut ArchiveWriter,
        board: &Board,
        team_id: &str,
    ) -> Result<(), AppError> {
        writer
            .start_entry(&format!("{}/board.jsonl", board.id))
            .await?;
        writer.write_line("board", board).await?;

        let blocks = self.blocks_for_board(&board.id).await?;
        // Two blocks may name the same file. The original then writes two entries of the
        // same name; this archive writer refuses a second entry of a name, and an
        // importer keeps one file per name anyway, so each file is written once.
        let mut files = Vec::new();
        let mut seen = HashSet::new();
        for block in &blocks {
            writer.write_line("block", block).await?;
            for filename in block.shown_files() {
                // An attachment block names its file as a client wrote it. A name that cannot
                // be a stored name would be read back as another entry of the archive,
                // or clash with one.
                if is_stored_name(&filename) && seen.insert(filename.clone()) {
                    files.push(filename);
                }
            }
        }

        for member in self.members_for_board(&board.id).await? {
            writer.write_line("boardMember", &member).await?;
        }

        for filename in &files {
            self.write_file(writer, &board.id, team_id, filename)
                .await?;
        }
        Ok(())
    }

    /// Writes a board file as its own archive entry; a missing file leaves an empty
    /// entry, as the original does.
    async fn write_file(
        &self,
        writer: &mut ArchiveWriter,
        board_id: &str,
        team_id: &str,
        filename: &str,
    ) -> Result<(), AppError> {
        writer
            .start_entry(&format!("{board_id}/{filename}"))
            .await?;
        let Some(mut stream) = self.open_stored_file(board_id, filename).await? else {
            tracing::error!(filename, team_id, board_id, "image file missing for export");
            return Ok(());
        };
        while let Some(chunk) = stream.next().await {
            let chunk = chunk
                .map_err(|error| AppError::internal(format_args!("cannot read file: {error}")))?;
            writer.write(&chunk).await?;
        }
        Ok(())
    }
}

/// The ZIP writer of an archive.
type Zip = ZipWriter<BufWriter<std::fs::File>>;

/// Writes an archive to a file. The writing and the compression are done off the
/// runtime's workers, a chunk at a time.
struct ArchiveWriter {
    /// The writer, away while a chunk is written; none after a failure. Boxed: it is
    /// large, and held across every wait of the export.
    zip: Option<Box<Zip>>,
    /// What is gathered for the current entry and not yet written.
    pending: Vec<u8>,
}

impl ArchiveWriter {
    fn new(file: std::fs::File) -> Self {
        Self {
            zip: Some(Box::new(ZipWriter::new(BufWriter::new(file)))),
            pending: Vec::new(),
        }
    }

    /// Starts a new entry, compressed with DEFLATE.
    async fn start_entry(&mut self, name: &str) -> Result<(), AppError> {
        self.flush().await?;
        let name = name.to_owned();
        self.blocking(move |zip| {
            let options =
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            zip.start_file(name, options).map_err(|error| {
                AppError::internal(format_args!("cannot create archive entry: {error}"))
            })
        })
        .await
    }

    /// Writes bytes to the current entry.
    async fn write(&mut self, bytes: &[u8]) -> Result<(), AppError> {
        self.pending.extend_from_slice(bytes);
        if self.pending.len() >= WRITE_CHUNK_BYTES {
            self.flush().await?;
        }
        Ok(())
    }

    /// Writes one JSONL line: an [`ArchiveLine`] wrapping `value` under `line_type`,
    /// followed by a newline.
    async fn write_line(
        &mut self,
        line_type: &str,
        value: &impl Serialize,
    ) -> Result<(), AppError> {
        let data = serde_json::to_value(value).map_err(|error| {
            AppError::internal(format_args!("cannot encode archive line: {error}"))
        })?;
        let line = ArchiveLine {
            line_type: line_type.to_owned(),
            data,
        };
        let mut bytes = serde_json::to_vec(&line).map_err(|error| {
            AppError::internal(format_args!("cannot encode archive line: {error}"))
        })?;
        bytes.push(b'\n');
        self.write(&bytes).await
    }

    /// Writes what is gathered.
    async fn flush(&mut self) -> Result<(), AppError> {
        if self.pending.is_empty() {
            return Ok(());
        }
        let bytes = std::mem::take(&mut self.pending);
        self.blocking(move |zip| {
            zip.write_all(&bytes).map_err(|error| {
                AppError::internal(format_args!("cannot write archive entry: {error}"))
            })
        })
        .await
    }

    /// Ends the archive: its directory, and what is buffered.
    async fn finish(mut self) -> Result<(), AppError> {
        self.flush().await?;
        let zip = self
            .zip
            .take()
            .ok_or_else(|| AppError::internal("the archive writer failed before"))?;
        tokio::task::spawn_blocking(move || -> zip::result::ZipResult<()> {
            (*zip).finish()?.flush()?;
            Ok(())
        })
        .await
        .map_err(|error| AppError::internal(format_args!("archive task failed: {error}")))?
        .map_err(|error| AppError::internal(format_args!("cannot finish archive: {error}")))
    }

    /// Runs `work` on the writer off the runtime's workers.
    async fn blocking(
        &mut self,
        work: impl FnOnce(&mut Zip) -> Result<(), AppError> + Send + 'static,
    ) -> Result<(), AppError> {
        let mut zip = self
            .zip
            .take()
            .ok_or_else(|| AppError::internal("the archive writer failed before"))?;
        let zip = tokio::task::spawn_blocking(move || {
            // A writer that failed is dropped here, off the runtime's workers: dropping
            // one ends the archive, which writes.
            work(&mut zip).map(|()| zip)
        })
        .await
        .map_err(|error| AppError::internal(format_args!("archive task failed: {error}")))??;
        self.zip = Some(zip);
        Ok(())
    }
}

impl Drop for ArchiveWriter {
    /// A writer left unfinished, as when reading a file failed, ends its archive when it
    /// is dropped, which writes: that is done off the runtime's workers.
    fn drop(&mut self) {
        if let Some(zip) = self.zip.take() {
            drop(tokio::task::spawn_blocking(move || drop(zip)));
        }
    }
}

/// A temporary file that cannot be written or read: the server's failure.
fn io_error(error: &std::io::Error) -> AppError {
    AppError::internal(format_args!("cannot write the archive: {error}"))
}

/// Whether a name a block shows can be the stored name of a file, and so an entry of the
/// board's directory: one path segment, and not a name the archive itself uses.
fn is_stored_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains(['/', '\\'])
        && !matches!(name, "." | ".." | "board.jsonl" | "version.json")
}
