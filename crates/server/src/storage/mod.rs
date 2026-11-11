//! File storage: where uploaded images and attachments live.
//!
//! Two backends exist, selected by the `filesdriver` setting: a directory on the local
//! disk and an S3-compatible object store. Both store a file under the same relative path
//! the original server uses, so an existing files directory or bucket keeps working.

#[cfg(feature = "test-util")]
mod faults;
mod local;
mod s3;

use std::ops::Range;

use bytes::Bytes;
use futures_util::stream::BoxStream;
use thiserror::Error;
use tokio::io::AsyncRead;

#[cfg(feature = "test-util")]
pub use faults::{Faults, FaultyOp};
pub use local::{LocalFiles, WRITING_DIR};
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

/// A stored file as a listing of the storage finds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredObject {
    /// Its place: the path a record names it by.
    pub path: String,
    /// When it was last written, in milliseconds since the Unix epoch.
    pub modified_at: i64,
}

/// Every stored file, as a stream: read as it is taken, a page or a directory at a
/// time, in no order.
pub type ObjectListing = BoxStream<'static, Result<StoredObject, StorageError>>;

/// The configured file backend.
#[derive(Debug, Clone)]
pub enum FileBackend {
    /// A directory on the local disk.
    Local(LocalFiles),
    /// An S3-compatible object store.
    S3(S3Files),
    /// A backend that fails when a test asks it to (the feature `test-util`, under
    /// which every backend made from the configuration is one).
    #[cfg(feature = "test-util")]
    Faulty(Box<Self>, std::sync::Arc<Faults>),
}

impl FileBackend {
    /// Creates the backend that the configuration selects.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Config`] if `filesdriver` names no known backend or the
    /// S3 settings cannot be used.
    pub fn from_config(config: &Configuration) -> Result<Self, StorageError> {
        let backend = match config.files_driver.as_str() {
            "local" => Self::Local(LocalFiles::new(&config.files_path)),
            "amazons3" => Self::S3(S3Files::new(&config.files_s3_config)?),
            other => {
                return Err(StorageError::Config(format!(
                    "unknown filesdriver {other:?}"
                )));
            }
        };
        #[cfg(feature = "test-util")]
        let backend = Self::Faulty(Box::new(backend), std::sync::Arc::default());
        Ok(backend)
    }

    /// The failures a test can ask this backend for, if it is one that takes them.
    #[cfg(feature = "test-util")]
    pub fn faults(&self) -> Option<&Faults> {
        match self {
            Self::Faulty(_, faults) => Some(faults),
            _ => None,
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
            #[cfg(feature = "test-util")]
            Self::Faulty(inner, faults) => {
                faults.check(FaultyOp::Write, path)?;
                let written = Box::pin(inner.write(path, reader)).await?;
                faults.check(FaultyOp::WriteAnswerLost, path)?;
                Ok(written)
            }
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
            #[cfg(feature = "test-util")]
            Self::Faulty(inner, _) => Box::pin(inner.size(path)).await,
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
            #[cfg(feature = "test-util")]
            Self::Faulty(inner, _) => Box::pin(inner.read(path, range)).await,
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
            #[cfg(feature = "test-util")]
            Self::Faulty(inner, faults) => {
                faults.check(FaultyOp::Copy, to)?;
                Box::pin(inner.copy(from, to)).await
            }
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
            #[cfg(feature = "test-util")]
            Self::Faulty(inner, faults) => {
                faults.check(FaultyOp::Delete, path)?;
                Box::pin(inner.delete(path)).await
            }
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
            #[cfg(feature = "test-util")]
            Self::Faulty(inner, _) => Box::pin(inner.rename(from, to)).await,
        }
    }

    /// Lists every stored file: on the local disk, the files under the root but for
    /// the directories whose name starts with a dot (the files being written, the
    /// temporary files); in the object store, the objects under the path prefix.
    pub fn list(&self) -> ObjectListing {
        match self {
            Self::Local(files) => files.list(),
            Self::S3(files) => files.list(),
            #[cfg(feature = "test-util")]
            Self::Faulty(inner, _) => inner.list(),
        }
    }

    /// The object store that takes uploads part by part across requests, if the backend
    /// is one: the local disk is not (`docs/s6-design.md` 5.6, 6).
    pub fn multipart(&self) -> Option<&S3Files> {
        match self {
            Self::Local(_) => None,
            Self::S3(files) => Some(files),
            #[cfg(feature = "test-util")]
            Self::Faulty(inner, _) => inner.multipart(),
        }
    }
}
