//! The S3 file backend.

use std::ops::Range;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{StreamExt as _, TryStreamExt as _};
use object_store::aws::{AmazonS3Builder, AmazonS3ConfigKey};
use object_store::buffered::BufWriter;
use object_store::path::Path;
use object_store::{ClientOptions, GetOptions, GetRange, ObjectStore, ObjectStoreExt as _};
use tokio::io::{AsyncRead, AsyncWriteExt as _};

use super::{FileStream, StorageError};
use crate::config::AmazonS3Config;

/// Files kept in a bucket of an S3-compatible object store, the `filess3config` setting.
///
/// A stored path such as `boards/20240131/7abc.png` is the object key, after the
/// configured path prefix.
#[derive(Debug, Clone)]
pub struct S3Files {
    store: Arc<dyn ObjectStore>,
    prefix: String,
}

impl S3Files {
    /// Creates a backend from the S3 settings.
    ///
    /// With an empty access key the credentials come from the environment: the standard
    /// `AWS_*` variables, a web identity token, or the instance metadata service.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Config`] if no bucket is set, version 2 signing is asked
    /// for, or the object store client cannot be built from the settings.
    pub fn new(config: &AmazonS3Config) -> Result<Self, StorageError> {
        if config.bucket.is_empty() {
            return Err(StorageError::Config(
                "filess3config.Bucket is empty".to_owned(),
            ));
        }
        if config.sign_v2 {
            return Err(StorageError::Config(
                "filess3config.SignV2 is not supported; requests are signed with version 4"
                    .to_owned(),
            ));
        }

        let mut builder = if config.access_key_id.is_empty() {
            AmazonS3Builder::from_env()
        } else {
            AmazonS3Builder::new()
                .with_access_key_id(&config.access_key_id)
                .with_secret_access_key(&config.secret_access_key)
        };
        builder = builder.with_bucket_name(&config.bucket);
        if !config.region.is_empty() {
            builder = builder.with_region(&config.region);
        }
        if !config.endpoint.is_empty() {
            // A custom endpoint means an S3-compatible server, which is addressed by path
            // (`host/bucket/key`), not by a bucket subdomain.
            let scheme = if config.ssl { "https" } else { "http" };
            builder = builder
                .with_endpoint(format!("{scheme}://{}", config.endpoint))
                .with_virtual_hosted_style_request(false);
        }
        builder = builder.with_allow_http(!config.ssl);
        if config.sse {
            // Server-side encryption with keys managed by the object store (SSE-S3).
            let key: AmazonS3ConfigKey = "aws_server_side_encryption"
                .parse()
                .map_err(|error: object_store::Error| StorageError::Config(error.to_string()))?;
            builder = builder.with_config(key, "AES256");
        }
        if let Ok(millis @ 1..) = u64::try_from(config.timeout) {
            builder = builder.with_client_options(
                ClientOptions::new().with_timeout(Duration::from_millis(millis)),
            );
        }

        let store = builder
            .build()
            .map_err(|error| StorageError::Config(error.to_string()))?;
        Ok(Self {
            store: Arc::new(store),
            prefix: config.path_prefix.trim_matches('/').to_owned(),
        })
    }

    /// The object key of a stored path: the configured prefix followed by the path.
    ///
    /// Characters that are not allowed in a key segment are percent-encoded, so a file
    /// with such a name gets a different key than the original server would give it.
    fn key(&self, path: &str) -> Result<Path, StorageError> {
        let has_parent_reference = path.split('/').any(|segment| segment == "..");
        if path.is_empty() || path.starts_with('/') || has_parent_reference {
            return Err(StorageError::InvalidPath(path.to_owned()));
        }
        Ok(if self.prefix.is_empty() {
            Path::from(path)
        } else {
            Path::from(format!("{}/{path}", self.prefix))
        })
    }

    pub(super) async fn write(
        &self,
        path: &str,
        reader: &mut (dyn AsyncRead + Send + Unpin),
    ) -> Result<u64, StorageError> {
        let mut writer = BufWriter::new(Arc::clone(&self.store), self.key(path)?);
        let written = match tokio::io::copy(reader, &mut writer).await {
            Ok(written) => written,
            Err(error) => {
                // A large object is uploaded in parts; without this they would stay in
                // the bucket.
                if let Err(abort) = writer.abort().await {
                    tracing::warn!(error = %abort, path, "cannot abort the unfinished upload");
                }
                return Err(error.into());
            }
        };
        writer.shutdown().await?;
        Ok(written)
    }

    pub(super) async fn size(&self, path: &str) -> Result<Option<u64>, StorageError> {
        match self.store.head(&self.key(path)?).await {
            Ok(meta) => Ok(Some(meta.size)),
            Err(object_store::Error::NotFound { .. }) => Ok(None),
            Err(error) => Err(StorageError::ObjectStore(error)),
        }
    }

    pub(super) async fn read(
        &self,
        path: &str,
        range: Option<Range<u64>>,
    ) -> Result<FileStream, StorageError> {
        let options = GetOptions {
            range: range.map(GetRange::Bounded),
            ..GetOptions::default()
        };
        let result = self
            .store
            .get_opts(&self.key(path)?, options)
            .await
            .map_err(|error| convert(error, path))?;
        Ok(result
            .into_stream()
            .map_err(StorageError::ObjectStore)
            .boxed())
    }

    pub(super) async fn copy(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.store
            .copy(&self.key(from)?, &self.key(to)?)
            .await
            .map_err(|error| convert(error, from))
    }

    pub(super) async fn delete(&self, path: &str) -> Result<(), StorageError> {
        match self.store.delete(&self.key(path)?).await {
            // What is gone is deleted.
            Ok(()) | Err(object_store::Error::NotFound { .. }) => Ok(()),
            Err(error) => Err(StorageError::ObjectStore(error)),
        }
    }

    pub(super) async fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.store
            .rename(&self.key(from)?, &self.key(to)?)
            .await
            .map_err(|error| convert(error, from))
    }
}

fn convert(error: object_store::Error, path: &str) -> StorageError {
    match error {
        object_store::Error::NotFound { .. } => StorageError::NotFound(path.to_owned()),
        other => StorageError::ObjectStore(other),
    }
}
