//! File storage: where uploaded images and attachments live.
//!
//! Two backends exist, selected by the `filesdriver` setting: a directory on the local
//! disk and an S3-compatible object store. Both store a file under the same relative path
//! the original server uses, so an existing files directory or bucket keeps working.
//!
//! A backend given the metrics (`FileBackend::measured`) measures every operation, by
//! the backend and the operation, at this boundary; but the listing, which is no
//! operation of a request's.

#[cfg(feature = "test-util")]
mod faults;
mod local;
mod measure;
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
use crate::metrics::{FilesBackend, FilesOperation, Metrics};

/// An error of a file backend.
#[derive(Debug, Error)]
pub enum StorageError {
    /// No file exists at the path.
    #[error("file not found: {0}")]
    NotFound(String),
    /// The path is not a place spelled the one way ([`fvoci_model::is_storage_path`]):
    /// one that could leave the storage root, or another spelling of a place.
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

/// The key the readiness check lists under in the object store. Nothing stores a file
/// at it (no stored path begins with a dot); that the list is answered is what counts.
const READY_KEY: &str = ".fvoci-ready";

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

    /// The same backend, measuring its operations into `metrics`.
    #[must_use]
    pub(crate) fn measured(self, metrics: Metrics) -> Self {
        match self {
            Self::Local(files) => {
                metrics.files_known(FilesBackend::Local);
                Self::Local(files.measured(metrics))
            }
            Self::S3(files) => {
                metrics.files_known(FilesBackend::S3);
                Self::S3(files.measured(metrics))
            }
            #[cfg(feature = "test-util")]
            Self::Faulty(inner, faults) => Self::Faulty(Box::new(inner.measured(metrics)), faults),
        }
    }

    /// The metrics the operations are measured into, and the backend they are of;
    /// `None` for a backend that only passes them on, measured by the one it wraps.
    fn metrics(&self) -> Option<(&Metrics, FilesBackend)> {
        match self {
            Self::Local(files) => Some((&files.metrics, FilesBackend::Local)),
            Self::S3(files) => Some((&files.metrics, FilesBackend::S3)),
            #[cfg(feature = "test-util")]
            Self::Faulty(..) => None,
        }
    }

    /// Runs `work`, which is `operation` on this backend, measured.
    async fn measure<T>(
        &self,
        operation: FilesOperation,
        work: impl Future<Output = Result<T, StorageError>>,
    ) -> Result<T, StorageError> {
        match self.metrics() {
            Some((metrics, backend)) => measure::measured(metrics, backend, operation, work).await,
            None => work.await,
        }
    }

    /// Stores everything `reader` yields at `path`, replacing any existing file, and
    /// returns the number of bytes written.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::InvalidPath`] for a path that is no place of the
    /// storage, or the backend's error if reading or writing fails.
    pub async fn write(
        &self,
        path: &str,
        reader: &mut (dyn AsyncRead + Send + Unpin),
    ) -> Result<u64, StorageError> {
        let Some(measuring) = self.metrics().and_then(|(metrics, backend)| {
            measure::Measuring::start(metrics, backend, FilesOperation::Put)
        }) else {
            return self.store(path, reader).await;
        };
        let mut input = measure::WatchedInput::new(reader);
        let result = self.store(path, &mut input).await;
        measuring.end(input.outcome(&result));
        result
    }

    /// Stores everything `reader` yields at `path`, unmeasured.
    async fn store(
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
    /// Returns [`StorageError::InvalidPath`] for a path that is no place of the
    /// storage, or the backend's error if the lookup fails.
    pub async fn size(&self, path: &str) -> Result<Option<u64>, StorageError> {
        self.measure(FilesOperation::Head, async {
            match self {
                Self::Local(files) => files.size(path).await,
                Self::S3(files) => files.size(path).await,
                #[cfg(feature = "test-util")]
                Self::Faulty(inner, _) => Box::pin(inner.size(path)).await,
            }
        })
        .await
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
    /// What is measured is the time until the bytes can be read; an error while they
    /// are is counted as a failure of the same operation.
    ///
    /// The range must lie within the file.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::NotFound`] if there is no file at `path`,
    /// [`StorageError::InvalidPath`] for a path that is no place of the storage, or the
    /// backend's error if the file cannot be opened.
    pub async fn read(
        &self,
        path: &str,
        range: Option<Range<u64>>,
    ) -> Result<FileStream, StorageError> {
        let stream = self
            .measure(FilesOperation::Get, async {
                match self {
                    Self::Local(files) => files.read(path, range).await,
                    Self::S3(files) => files.read(path, range).await,
                    #[cfg(feature = "test-util")]
                    Self::Faulty(inner, _) => Box::pin(inner.read(path, range)).await,
                }
            })
            .await?;
        Ok(match self.metrics() {
            Some((metrics, backend)) => measure::counting_failures(stream, metrics, backend),
            None => stream,
        })
    }

    /// Copies the file at `from` to `to`, replacing any existing file.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::NotFound`] if there is no file at `from`, or the backend's
    /// error if the copy fails.
    pub async fn copy(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.measure(FilesOperation::Copy, async {
            match self {
                Self::Local(files) => files.copy(from, to).await,
                Self::S3(files) => files.copy(from, to).await,
                #[cfg(feature = "test-util")]
                Self::Faulty(inner, faults) => {
                    faults.check(FaultyOp::Copy, to)?;
                    Box::pin(inner.copy(from, to)).await
                }
            }
        })
        .await
    }

    /// Removes the file at `path`. Removing a file that is not there succeeds.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::InvalidPath`] for a path that is no place of the
    /// storage, and the backend's error if the file cannot be removed.
    pub async fn delete(&self, path: &str) -> Result<(), StorageError> {
        self.measure(FilesOperation::Delete, async {
            match self {
                Self::Local(files) => files.delete(path).await,
                Self::S3(files) => files.delete(path).await,
                #[cfg(feature = "test-util")]
                Self::Faulty(inner, faults) => {
                    faults.check(FaultyOp::Delete, path)?;
                    Box::pin(inner.delete(path)).await
                }
            }
        })
        .await
    }

    /// Moves the file at `from` to `to`, replacing any existing file.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::NotFound`] if there is no file at `from`, or the backend's
    /// error if the move fails.
    pub async fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.measure(FilesOperation::Rename, async {
            match self {
                Self::Local(files) => files.rename(from, to).await,
                Self::S3(files) => files.rename(from, to).await,
                #[cfg(feature = "test-util")]
                Self::Faulty(inner, _) => Box::pin(inner.rename(from, to)).await,
            }
        })
        .await
    }

    /// Whether the storage can be used, for the readiness check, unmeasured: on the local
    /// disk, that the directory is one or is still to be made; on the object store, that
    /// the bucket is there and lists its objects under a key that need not be there, with
    /// the credentials.
    ///
    /// # Errors
    ///
    /// Returns the backend's error if the look fails.
    pub async fn check(&self) -> Result<(), StorageError> {
        match self {
            Self::Local(files) => files.check().await,
            Self::S3(files) => files.check(READY_KEY).await,
            #[cfg(feature = "test-util")]
            Self::Faulty(inner, _) => Box::pin(inner.check()).await,
        }
    }

    /// Lists every stored file: on the local disk, the files under the root but for
    /// those in a directory at the root whose name starts with a dot (the files being
    /// written, the temporary files), in a directory the server may not read, or whose
    /// name is no text; in the object store, the objects under the path prefix, by
    /// their keys as the object store encodes them.
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
