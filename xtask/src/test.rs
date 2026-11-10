//! `cargo xtask test`: every test, on SQLite and on PostgreSQL, and the S3 storage test.
//!
//! The containers are prepared first unless the environment already names the services:
//! `FVOCI_STORE_TEST_CONN_STRING` for PostgreSQL, `FVOCI_S3_TEST_ENDPOINT` together with
//! `FVOCI_S3_TEST_BUCKET` for the object store (for CI, where the services are given).

use std::path::Path;

use anyhow::Result;

use crate::Databases;
use crate::containers;
use crate::steps::{self, Step};

/// The variables the store's test support reads.
const DB_TYPE_ENV: &str = "FVOCI_STORE_TEST_DB_TYPE";
const CONN_STRING_ENV: &str = "FVOCI_STORE_TEST_CONN_STRING";

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
    let cargo = steps::cargo();
    let mut parts = Vec::new();

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

    if databases == Databases::All {
        parts.push(Step::new("S3 storage test", || {
            let (endpoint, bucket) = if let (Some(endpoint), Some(bucket)) =
                (env(S3_ENDPOINT_ENV), env(S3_BUCKET_ENV))
            {
                steps::say(&format!("    using the object store of {S3_ENDPOINT_ENV}"));
                (endpoint, bucket)
            } else {
                let (endpoint, bucket) = containers::prepare_object_store()?;
                (endpoint, bucket.to_owned())
            };
            steps::run(
                root,
                &cargo,
                &[
                    "test",
                    "--locked",
                    "--package",
                    "fvoci-server",
                    "--test",
                    "storage",
                    "--",
                    "--ignored",
                ],
                &[(S3_ENDPOINT_ENV, &endpoint), (S3_BUCKET_ENV, &bucket)],
            )
        }));
    }

    steps::run_all("test", parts)
}
