//! `cargo xtask test`: every test, on SQLite and on PostgreSQL, and the S3 storage test
//! with the uploads in parts that go to S3.
//!
//! The containers are prepared first unless the environment already names the services:
//! `FVOCI_STORE_TEST_CONN_STRING` for PostgreSQL, `FVOCI_S3_TEST_ENDPOINT` together with
//! `FVOCI_S3_TEST_BUCKET` for the object store (for CI, where the services are given).

use std::path::Path;

use anyhow::{Result, bail};

use crate::Databases;
use crate::containers;
use crate::steps::{self, Step};

/// The variables the store's test support reads.
const DB_TYPE_ENV: &str = "FVOCI_STORE_TEST_DB_TYPE";
const CONN_STRING_ENV: &str = "FVOCI_STORE_TEST_CONN_STRING";

/// The variable that makes the regression tests record instead of compare.
const REGRESSION_UPDATE_ENV: &str = "FVOCI_REGRESSION_UPDATE";

/// The variables the S3 storage test reads.
const S3_ENDPOINT_ENV: &str = "FVOCI_S3_TEST_ENDPOINT";
const S3_BUCKET_ENV: &str = "FVOCI_S3_TEST_BUCKET";

fn env(variable: &str) -> Option<String> {
    std::env::var(variable)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

/// Runs the tests on the chosen databases. Every part runs, also after one failed.
///
/// # Errors
///
/// Fails if a container cannot be prepared or a test run failed.
pub(crate) fn run(root: &Path, databases: Databases) -> Result<()> {
    // With it set the regression tests rewrite their snapshots instead of comparing,
    // and would do so once per database. This task verifies; it never records.
    if env(REGRESSION_UPDATE_ENV).is_some() {
        bail!(
            "{REGRESSION_UPDATE_ENV} is set: the regression tests would rewrite their \
             snapshots instead of checking them. Unset it; to record snapshots run \
             `{REGRESSION_UPDATE_ENV}=1 cargo test -p fvoci-server --test regression`"
        );
    }
    let cargo = steps::cargo();
    let mut parts = Vec::new();

    if databases == Databases::Postgres {
        parts.push(Step::skipped("tests on SQLite", "--database postgres"));
    }
    if matches!(databases, Databases::All | Databases::Sqlite) {
        parts.push(Step::new("tests on SQLite", || {
            // Whatever the caller's environment says, this run is the SQLite one.
            steps::run(
                root,
                &cargo,
                &["test", "--workspace", "--locked"],
                &[(DB_TYPE_ENV, "sqlite3")],
            )
        }));
    }

    if matches!(databases, Databases::All | Databases::Postgres) {
        parts.push(Step::new("tests on PostgreSQL", || {
            let url = match env(CONN_STRING_ENV) {
                Some(url) => {
                    steps::say(&format!("    using the PostgreSQL of {CONN_STRING_ENV}"));
                    url
                }
                None => containers::prepare_postgres()?,
            };
            steps::run(
                root,
                &cargo,
                &[
                    "test",
                    "--locked",
                    "--package",
                    "fvoci-store",
                    "--package",
                    "fvoci-server",
                ],
                &[(DB_TYPE_ENV, "postgres"), (CONN_STRING_ENV, &url)],
            )
        }));
    }

    if databases == Databases::Sqlite {
        parts.push(Step::skipped("tests on PostgreSQL", "--database sqlite"));
    }
    if databases != Databases::All {
        parts.push(Step::skipped("S3 storage test", "only with every database"));
    }
    if databases == Databases::All {
        parts.push(Step::new("S3 storage test", || s3_tests(root, &cargo)));
    }

    steps::run_all("test", parts)
}

/// The tests that need an S3-compatible server: the storage test, and the uploads in
/// parts that go to S3 as multipart uploads. Runs against the object store of the
/// environment, or the container prepared for them.
fn s3_tests(root: &Path, cargo: &std::ffi::OsStr) -> Result<()> {
    let (endpoint, bucket) =
        if let (Some(endpoint), Some(bucket)) = (env(S3_ENDPOINT_ENV), env(S3_BUCKET_ENV)) {
            steps::say(&format!("    using the object store of {S3_ENDPOINT_ENV}"));
            (endpoint, bucket)
        } else {
            let (endpoint, bucket) = containers::prepare_object_store()?;
            (endpoint, bucket.to_owned())
        };
    let s3 = [
        (S3_ENDPOINT_ENV, endpoint.as_str()),
        (S3_BUCKET_ENV, bucket.as_str()),
    ];
    let ignored = |test: &str, filter: Option<&str>| {
        let mut arguments = vec![
            "test",
            "--locked",
            "--package",
            "fvoci-server",
            "--test",
            test,
            "--",
            "--ignored",
        ];
        arguments.extend(filter);
        steps::run(root, cargo, &arguments, &s3)
    };
    ignored("storage", None)?;
    ignored("api", Some("with_s3"))
}
