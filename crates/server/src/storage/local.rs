//! The local-disk file backend.

use std::io::SeekFrom;
use std::ops::Range;
use std::path::{Component, Path, PathBuf};

use futures_util::{StreamExt as _, TryStreamExt as _};
use tokio::fs;
use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncSeekExt as _, AsyncWriteExt as _};
use tokio_util::io::ReaderStream;

use super::{FileStream, StorageError};
use crate::metrics::Metrics;

/// Files kept in a directory on the local disk, the `filespath` setting.
///
/// A stored path such as `boards/20240131/7abc.png` is that relative path under the
/// directory, exactly as the original server lays files out.
#[derive(Debug, Clone)]
pub struct LocalFiles {
    root: PathBuf,
    /// What the operations are measured into ([`super::FileBackend::measured`]).
    pub(super) metrics: Metrics,
}

/// The directory under the root, in which files are written before they are renamed
/// into their place. No stored path begins with a dot.
pub const WRITING_DIR: &str = ".tmp/writing";

/// Writes everything `reader` yields to a new file at `path`, and returns the number of
/// bytes written.
async fn write_file(
    path: &Path,
    reader: &mut (dyn AsyncRead + Send + Unpin),
) -> std::io::Result<u64> {
    let mut file = fs::File::create(path).await?;
    let written = tokio::io::copy(reader, &mut file).await?;
    file.flush().await?;
    // On the disk before the rename makes it the file: after a power loss the name does
    // not point at nothing.
    file.sync_all().await?;
    Ok(written)
}

impl LocalFiles {
    /// Creates a backend rooted at `directory`. The directory is created on first write.
    pub fn new(directory: impl AsRef<Path>) -> Self {
        Self {
            root: directory.as_ref().to_path_buf(),
            metrics: Metrics::none(),
        }
    }

    /// The same backend, measuring its operations into `metrics`.
    pub(super) fn measured(self, metrics: Metrics) -> Self {
        Self { metrics, ..self }
    }

    /// Resolves a stored path to a path under the root.
    ///
    /// Only plain relative paths are accepted. The original joins whatever it is given
    /// onto the root; here an absolute path or a `..` component is refused, so no stored
    /// path can ever name a file outside the root.
    fn resolve(&self, path: &str) -> Result<PathBuf, StorageError> {
        let relative = Path::new(path);
        let plain = !path.is_empty()
            && relative
                .components()
                .all(|component| matches!(component, Component::Normal(_) | Component::CurDir));
        if plain {
            Ok(self.root.join(relative))
        } else {
            Err(StorageError::InvalidPath(path.to_owned()))
        }
    }

    pub(super) async fn write(
        &self,
        path: &str,
        reader: &mut (dyn AsyncRead + Send + Unpin),
    ) -> Result<u64, StorageError> {
        let target = self.resolve(path)?;
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).await?;
        }
        // The file is written under the root and renamed into its place, which on one
        // file system is atomic: a reader never finds half a file, and a write that fails
        // leaves nothing at the path. What a crash leaves is in WRITING_DIR, which the
        // server empties when it starts.
        let writing = self.root.join(WRITING_DIR);
        fs::create_dir_all(&writing).await?;
        let partial = writing.join(crate::auth::secret::new_secret());
        let written = match write_file(&partial, reader).await {
            Ok(written) => fs::rename(&partial, &target).await.map(|()| written),
            Err(error) => Err(error),
        };
        if written.is_err() {
            match fs::remove_file(&partial).await {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => tracing::warn!(%error, "a partial file could not be removed"),
            }
        }
        Ok(written?)
    }

    pub(super) async fn size(&self, path: &str) -> Result<Option<u64>, StorageError> {
        match fs::metadata(self.resolve(path)?).await {
            Ok(metadata) if metadata.is_file() => Ok(Some(metadata.len())),
            Ok(_) => Ok(None),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub(super) async fn read(
        &self,
        path: &str,
        range: Option<Range<u64>>,
    ) -> Result<FileStream, StorageError> {
        let mut file = fs::File::open(self.resolve(path)?)
            .await
            .map_err(|error| not_found_or_io(error, path))?;
        match range {
            None => Ok(ReaderStream::new(file).map_err(StorageError::from).boxed()),
            Some(range) => {
                file.seek(SeekFrom::Start(range.start)).await?;
                let length = range.end.saturating_sub(range.start);
                Ok(ReaderStream::new(file.take(length))
                    .map_err(StorageError::from)
                    .boxed())
            }
        }
    }

    pub(super) async fn copy(&self, from: &str, to: &str) -> Result<(), StorageError> {
        let (source, target) = (self.resolve(from)?, self.resolve(to)?);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::copy(source, target)
            .await
            .map_err(|error| not_found_or_io(error, from))?;
        Ok(())
    }

    pub(super) async fn delete(&self, path: &str) -> Result<(), StorageError> {
        match fs::remove_file(self.resolve(path)?).await {
            Ok(()) => Ok(()),
            // What is gone is deleted.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(StorageError::Io(error)),
        }
    }

    pub(super) async fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        let (source, target) = (self.resolve(from)?, self.resolve(to)?);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::rename(source, target)
            .await
            .map_err(|error| not_found_or_io(error, from))
    }
}

fn not_found_or_io(error: std::io::Error, path: &str) -> StorageError {
    if error.kind() == std::io::ErrorKind::NotFound {
        StorageError::NotFound(path.to_owned())
    } else {
        StorageError::Io(error)
    }
}
