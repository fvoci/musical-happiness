//! Exporting boards to the version 2 archive format.
//!
//! Ports `server/app/export.go`. An archive is a ZIP holding `version.json` and, per
//! board, a directory with a `board.jsonl` and the board's files.

use std::io::{Cursor, Write as _};

use futures_util::StreamExt as _;
use fvoci_model::{ArchiveHeader, ArchiveLine, Block, BlockType, Board};
use serde::Serialize;
use serde_json::Value;
use zip::write::{SimpleFileOptions, ZipWriter};

use super::App;
use crate::auth::now_millis;
use crate::error::AppError;

/// The archive format version this server writes.
pub(super) const ARCHIVE_VERSION: i64 = 2;

impl App {
    /// Builds an archive of the given boards and returns its bytes.
    ///
    /// Each board is written with its blocks and members; a file a block refers to is
    /// added if it exists and left as an empty entry if it is missing, as the original
    /// does.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if a board does not exist, or a server error if an image
    /// or attachment block names no file or the archive cannot be written.
    pub async fn export_archive(
        &self,
        team_id: &str,
        board_ids: &[String],
    ) -> Result<Vec<u8>, AppError> {
        let mut boards = Vec::with_capacity(board_ids.len());
        for id in board_ids {
            boards.push(self.store().get_board(id).await.map_err(|error| {
                AppError::from(error).context(format_args!("could not fetch board {id}"))
            })?);
        }

        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        write_version(&mut writer)?;
        for board in &boards {
            self.write_board(&mut writer, board, team_id).await?;
        }
        let cursor = writer
            .finish()
            .map_err(|error| AppError::internal(format_args!("cannot finish archive: {error}")))?;
        Ok(cursor.into_inner())
    }

    /// Writes one board's directory: its `board.jsonl` and its files.
    async fn write_board(
        &self,
        writer: &mut Writer,
        board: &Board,
        team_id: &str,
    ) -> Result<(), AppError> {
        start_entry(writer, &format!("{}/board.jsonl", board.id))?;
        write_line(writer, "board", board)?;

        let blocks = self.blocks_for_board(&board.id).await?;
        // Two blocks may name the same file. The original then writes two entries of the
        // same name; this archive writer refuses a second entry of a name, and an
        // importer keeps one file per name anyway, so each file is written once.
        let mut files = Vec::new();
        for block in &blocks {
            write_line(writer, "block", block)?;
            if block.block_type == BlockType::Image || block.block_type == BlockType::Attachment {
                let filename = extract_filename(block)?;
                if !files.contains(&filename) {
                    files.push(filename);
                }
            }
        }

        for member in self.members_for_board(&board.id).await? {
            write_line(writer, "boardMember", &member)?;
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
        writer: &mut Writer,
        board_id: &str,
        team_id: &str,
        filename: &str,
    ) -> Result<(), AppError> {
        start_entry(writer, &format!("{board_id}/{filename}"))?;
        let Some(mut stream) = self.open_stored_file(team_id, board_id, filename).await? else {
            tracing::error!(filename, team_id, board_id, "image file missing for export");
            return Ok(());
        };
        while let Some(chunk) = stream.next().await {
            let chunk = chunk
                .map_err(|error| AppError::internal(format_args!("cannot read file: {error}")))?;
            write_bytes(writer, &chunk)?;
        }
        Ok(())
    }
}

/// The ZIP writer the archive is built into.
type Writer = ZipWriter<Cursor<Vec<u8>>>;

/// Writes `version.json`.
fn write_version(writer: &mut Writer) -> Result<(), AppError> {
    let header = ArchiveHeader {
        version: ARCHIVE_VERSION,
        date: now_millis(),
    };
    start_entry(writer, "version.json")?;
    let bytes = serde_json::to_vec(&header).map_err(|error| {
        AppError::internal(format_args!("cannot write archive header: {error}"))
    })?;
    write_bytes(writer, &bytes)
}

/// Starts a new archive entry, compressed with DEFLATE.
fn start_entry(writer: &mut Writer, name: &str) -> Result<(), AppError> {
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    writer
        .start_file(name, options)
        .map_err(|error| AppError::internal(format_args!("cannot create archive entry: {error}")))
}

/// Writes bytes to the current archive entry.
fn write_bytes(writer: &mut Writer, bytes: &[u8]) -> Result<(), AppError> {
    writer
        .write_all(bytes)
        .map_err(|error| AppError::internal(format_args!("cannot write archive entry: {error}")))
}

/// Writes one JSONL line: an [`ArchiveLine`] wrapping `value` under `line_type`,
/// followed by a newline.
fn write_line(
    writer: &mut Writer,
    line_type: &str,
    value: &impl Serialize,
) -> Result<(), AppError> {
    let data = serde_json::to_value(value)
        .map_err(|error| AppError::internal(format_args!("cannot encode archive line: {error}")))?;
    let line = ArchiveLine {
        line_type: line_type.to_owned(),
        data,
    };
    let mut bytes = serde_json::to_vec(&line)
        .map_err(|error| AppError::internal(format_args!("cannot encode archive line: {error}")))?;
    bytes.push(b'\n');
    write_bytes(writer, &bytes)
}

/// The file an image or attachment block refers to: its `fileId`, or failing that its
/// `attachmentId`. A block with neither is invalid.
fn extract_filename(block: &Block) -> Result<String, AppError> {
    for key in ["fileId", "attachmentId"] {
        if let Some(Value::String(value)) = block.fields.get(key) {
            return Ok(value.clone());
        }
    }
    Err(AppError::internal("invalid image block"))
}
