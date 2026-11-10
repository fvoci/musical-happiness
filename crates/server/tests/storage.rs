//! Behavior of the file backends. The original has no tests for its file storage (it
//! mocks the host application's); these pin what the server relies on, and run the same
//! checks against the local backend and an S3-compatible server.
//!
//! The S3 test is ignored by default. To run it, start an S3-compatible server, create a
//! bucket, set `FVOCI_S3_TEST_ENDPOINT` (host:port), `FVOCI_S3_TEST_BUCKET` and, if the
//! server checks them, `FVOCI_S3_TEST_ACCESS_KEY`, `FVOCI_S3_TEST_SECRET_KEY`,
//! `FVOCI_S3_TEST_REGION` and `FVOCI_S3_TEST_SSL=true`, and pass `--ignored`.

use std::path::PathBuf;

use futures_util::TryStreamExt as _;
use fvoci_model::{IdKind, new_id};
use fvoci_server::config::AmazonS3Config;
use fvoci_server::storage::{FileBackend, LocalFiles, S3Files, StorageError};

async fn write(backend: &FileBackend, path: &str, content: &[u8]) -> u64 {
    let mut reader = content;
    backend
        .write(path, &mut reader)
        .await
        .unwrap_or_else(|error| panic!("writing {path}: {error}"))
}

async fn read(backend: &FileBackend, path: &str, range: Option<std::ops::Range<u64>>) -> Vec<u8> {
    let stream = backend
        .read(path, range)
        .await
        .unwrap_or_else(|error| panic!("opening {path}: {error}"));
    let chunks: Vec<bytes::Bytes> = stream
        .try_collect()
        .await
        .unwrap_or_else(|error| panic!("reading {path}: {error}"));
    chunks.concat()
}

/// Asserts that the file at `path` holds exactly `want`, read whole or by `range`.
async fn assert_content(
    backend: &FileBackend,
    path: &str,
    range: Option<std::ops::Range<u64>>,
    want: &[u8],
) {
    let got = read(backend, path, range.clone()).await;
    assert!(
        got == want,
        "{path} range {range:?}: got {} bytes, want {}",
        got.len(),
        want.len()
    );
}

/// The checks every backend must pass. `dir` is a path prefix unique to this run.
async fn exercise(backend: &FileBackend, dir: &str) {
    let path = format!("{dir}/boards/20240131/7abc.png");
    read_and_write(backend, &path, dir).await;
    copy_and_rename(backend, &path, dir).await;
    large_file(backend, dir).await;
    refused_paths(backend, &path).await;
}

async fn read_and_write(backend: &FileBackend, path: &str, dir: &str) {
    // Nothing is there yet.
    let size = backend.size(path).await.expect("size of a missing file");
    assert_eq!(size, None, "a missing file has no size");
    assert!(
        !backend.exists(path).await.expect("exists"),
        "{path} should not exist yet"
    );
    let missing = backend
        .read(path, None)
        .await
        .err()
        .expect("reading a missing file fails");
    assert!(
        matches!(missing, StorageError::NotFound(_)),
        "got {missing:?}"
    );

    // Write creates the directories on the way and reports the size.
    let content = b"0123456789abcdefghij";
    assert_eq!(write(backend, path, content).await, 20, "bytes written");
    assert_eq!(
        backend.size(path).await.expect("size"),
        Some(20),
        "size after the write"
    );
    assert!(
        backend.exists(path).await.expect("exists"),
        "{path} should exist"
    );
    assert_content(backend, path, None, content).await;

    // Ranges are byte offsets, end exclusive.
    assert_content(backend, path, Some(0..5), b"01234").await;
    assert_content(backend, path, Some(10..20), b"abcdefghij").await;
    assert_content(backend, path, Some(19..20), b"j").await;

    // Writing again replaces the file.
    assert_eq!(
        write(backend, path, b"new").await,
        3,
        "bytes written by the rewrite"
    );
    assert_content(backend, path, None, b"new").await;

    // An empty file is a file.
    let empty = format!("{dir}/empty.bin");
    assert_eq!(
        write(backend, &empty, b"").await,
        0,
        "bytes written for an empty file"
    );
    assert_eq!(
        backend.size(&empty).await.expect("size"),
        Some(0),
        "size of an empty file"
    );
    assert_content(backend, &empty, None, b"").await;
}

/// Expects `path` to hold `new`, as [`read_and_write`] leaves it.
async fn copy_and_rename(backend: &FileBackend, path: &str, dir: &str) {
    // Copy keeps the source; the target may be in a directory that does not exist yet.
    let copy = format!("{dir}/0/board-1/7copy.png");
    backend.copy(path, &copy).await.expect("copy");
    assert_content(backend, &copy, None, b"new").await;
    assert_content(backend, path, None, b"new").await;

    // Rename moves the file.
    let moved = format!("{dir}/0/board-2/7moved.png");
    backend.rename(&copy, &moved).await.expect("rename");
    assert_content(backend, &moved, None, b"new").await;
    assert!(
        !backend.exists(&copy).await.expect("exists"),
        "the source of a rename is gone"
    );

    // Copying or moving a missing file fails as not found.
    let gone = format!("{dir}/gone.png");
    let error = backend
        .copy(&gone, &copy)
        .await
        .expect_err("copy of a missing file");
    assert!(matches!(error, StorageError::NotFound(_)), "got {error:?}");
    let error = backend
        .rename(&gone, &copy)
        .await
        .expect_err("rename of a missing file");
    assert!(matches!(error, StorageError::NotFound(_)), "got {error:?}");
}

/// A large file survives the streaming write path.
async fn large_file(backend: &FileBackend, dir: &str) {
    let large: Vec<u8> = (0..3_000_000_u32)
        .map(|n| u8::try_from(n % 251).expect("a remainder below 251 fits a byte"))
        .collect();
    let path = format!("{dir}/large.bin");
    assert_eq!(
        write(backend, &path, &large).await,
        3_000_000,
        "bytes written for the large file"
    );
    assert_content(
        backend,
        &path,
        Some(2_999_990..3_000_000),
        &large[2_999_990..],
    )
    .await;
    assert_content(backend, &path, None, &large).await;
}

/// Paths that could leave the storage root are refused.
async fn refused_paths(backend: &FileBackend, existing: &str) {
    for bad in ["", "/etc/passwd", "../outside.png", "a/../../outside.png"] {
        let error = backend.size(bad).await.expect_err(bad);
        assert!(
            matches!(error, StorageError::InvalidPath(_)),
            "{bad:?}: got {error:?}"
        );
        let mut reader: &[u8] = b"x";
        let error = backend.write(bad, &mut reader).await.expect_err(bad);
        assert!(
            matches!(error, StorageError::InvalidPath(_)),
            "{bad:?}: got {error:?}"
        );
        let error = backend.copy(existing, bad).await.expect_err(bad);
        assert!(
            matches!(error, StorageError::InvalidPath(_)),
            "{bad:?}: got {error:?}"
        );
    }
}

#[tokio::test]
async fn local_backend() {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("storage-tests")
        .join(new_id(IdKind::None));
    let backend = FileBackend::Local(LocalFiles::new(&root));

    exercise(&backend, "run").await;

    // The local backend lays files out exactly as given, under its root.
    assert!(
        root.join("run/boards/20240131/7abc.png").is_file(),
        "the file is under the root"
    );
    // A file name is taken literally: no part of it is encoded.
    write(&backend, "run/a b#1.png", b"x").await;
    assert!(
        root.join("run/a b#1.png").is_file(),
        "the name is kept as it is"
    );

    std::fs::remove_dir_all(&root).expect("the test directory can be removed");
}

fn s3_config() -> AmazonS3Config {
    let var = |name: &str| std::env::var(name).unwrap_or_default();
    AmazonS3Config {
        access_key_id: std::env::var("FVOCI_S3_TEST_ACCESS_KEY")
            .unwrap_or_else(|_| "any".to_owned()),
        secret_access_key: std::env::var("FVOCI_S3_TEST_SECRET_KEY")
            .unwrap_or_else(|_| "any".to_owned()),
        bucket: var("FVOCI_S3_TEST_BUCKET"),
        path_prefix: "storage-tests".to_owned(),
        region: std::env::var("FVOCI_S3_TEST_REGION").unwrap_or_else(|_| "us-east-1".to_owned()),
        endpoint: var("FVOCI_S3_TEST_ENDPOINT"),
        ssl: var("FVOCI_S3_TEST_SSL") == "true",
        ..AmazonS3Config::default()
    }
}

#[tokio::test]
#[ignore = "needs an S3-compatible server; see the module documentation"]
async fn s3_backend() {
    let config = s3_config();
    assert!(
        !config.endpoint.is_empty() && !config.bucket.is_empty(),
        "set FVOCI_S3_TEST_ENDPOINT and FVOCI_S3_TEST_BUCKET to run this test"
    );
    let backend = FileBackend::S3(S3Files::new(&config).expect("the S3 settings are usable"));

    // Objects of earlier runs stay in the bucket; a fresh prefix keeps runs apart.
    exercise(&backend, &new_id(IdKind::None)).await;
}

#[test]
fn s3_settings_are_checked() {
    let error = S3Files::new(&AmazonS3Config::default()).expect_err("no bucket");
    assert!(matches!(error, StorageError::Config(_)), "got {error:?}");

    let sign_v2 = AmazonS3Config {
        bucket: "b".to_owned(),
        sign_v2: true,
        ..AmazonS3Config::default()
    };
    let error = S3Files::new(&sign_v2).expect_err("version 2 signing");
    assert!(error.to_string().contains("SignV2"), "got {error}");
}
