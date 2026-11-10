//! File storage: where uploaded images and attachments live.
//!
//! Two backends exist, selected by the `filesdriver` setting: a directory on the local
//! disk and an S3-compatible object store. Both store a file under the same relative path
//! the original server uses, so an existing files directory or bucket keeps working.

mod local;
mod s3;

use std::ops::Range;

use bytes::Bytes;
use futures_util::stream::BoxStream;
use thiserror::Error;
use tokio::io::AsyncRead;

pub use local::LocalFiles;
pub use s3::S3Files;

use crate::config::Configuration;

/// An error of a file backend.
#[derive(Debug, Error)]
pub enum StorageError {
    /// No file exists at the path.
    #[error("file not found: {0}")]
    NotFound(String),
    /// The path is absolute or has a `..` component, and so could leave the storage root.
    #[error("invalid file path: {0}")]
    InvalidPath(String),
    /// The backend cannot be set up from the configuration.
    #[error("invalid file storage configuration: {0}")]
    Config(String),
    /// A local file operation failed.
    #[error("file storage I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// The object store reported an error.
    #[error("object store error: {0}")]
    ObjectStore(#[source] object_store::Error),
}

/// The bytes of a stored file, as a stream of chunks.
pub type FileStream = BoxStream<'static, Result<Bytes, StorageError>>;

/// The configured file backend.
#[derive(Debug, Clone)]
pub enum FileBackend {
    /// A directory on the local disk.
    Local(LocalFiles),
    /// An S3-compatible object store.
    S3(S3Files),
}

impl FileBackend {
    /// Creates the backend that the configuration selects.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Config`] if `filesdriver` names no known backend or the
    /// S3 settings cannot be used.
    pub fn from_config(config: &Configuration) -> Result<Self, StorageError> {
        match config.files_driver.as_str() {
            "local" => Ok(Self::Local(LocalFiles::new(&config.files_path))),
            "amazons3" => Ok(Self::S3(S3Files::new(&config.files_s3_config)?)),
            other => Err(StorageError::Config(format!(
                "unknown filesdriver {other:?}"
            ))),
        }
    }

    /// Stores everything `reader` yields at `path`, replacing any existing file, and
    /// returns the number of bytes written.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::InvalidPath`] for a path that could leave the storage
    /// root, or the backend's error if reading or writing fails.
    pub async fn write(
        &self,
        path: &str,
        reader: &mut (dyn AsyncRead + Send + Unpin),
    ) -> Result<u64, StorageError> {
        match self {
            Self::Local(files) => files.write(path, reader).await,
            Self::S3(files) => files.write(path, reader).await,
        }
    }

    /// Returns the size in bytes of the file at `path`, or `None` if there is none.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::InvalidPath`] for a path that could leave the storage
    /// root, or the backend's error if the lookup fails.
    pub async fn size(&self, path: &str) -> Result<Option<u64>, StorageError> {
        match self {
            Self::Local(files) => files.size(path).await,
            Self::S3(files) => files.size(path).await,
        }
    }

    /// Returns `true` if a file exists at `path`.
    ///
    /// # Errors
    ///
    /// The errors of [`FileBackend::size`].
    pub async fn exists(&self, path: &str) -> Result<bool, StorageError> {
        Ok(self.size(path).await?.is_some())
    }

    /// Opens the file at `path` and returns the given byte range of it, or all of it.
    ///
    /// The range must lie within the file.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::NotFound`] if there is no file at `path`,
    /// [`StorageError::InvalidPath`] for a path that could leave the storage root, or the
    /// backend's error if the file cannot be opened.
    pub async fn read(
        &self,
        path: &str,
        range: Option<Range<u64>>,
    ) -> Result<FileStream, StorageError> {
        match self {
            Self::Local(files) => files.read(path, range).await,
            Self::S3(files) => files.read(path, range).await,
        }
    }

    /// Copies the file at `from` to `to`, replacing any existing file.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::NotFound`] if there is no file at `from`, or the backend's
    /// error if the copy fails.
    pub async fn copy(&self, from: &str, to: &str) -> Result<(), StorageError> {
        match self {
            Self::Local(files) => files.copy(from, to).await,
            Self::S3(files) => files.copy(from, to).await,
        }
    }

    /// Removes the file at `path`. Removing a file that is not there succeeds.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::InvalidPath`] for a path that could leave the storage
    /// root, and the backend's error if the file cannot be removed.
    pub async fn delete(&self, path: &str) -> Result<(), StorageError> {
        match self {
            Self::Local(files) => files.delete(path).await,
            Self::S3(files) => files.delete(path).await,
        }
    }

    /// Moves the file at `from` to `to`, replacing any existing file.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::NotFound`] if there is no file at `from`, or the backend's
    /// error if the move fails.
    pub async fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        match self {
            Self::Local(files) => files.rename(from, to).await,
            Self::S3(files) => files.rename(from, to).await,
        }
    }
}
