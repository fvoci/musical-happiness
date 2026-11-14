//! The versions kept of the bodies of cards (`body_versions`), and who wrote the body
//! up to each (`body_version_editors`).
//!
//! The server keeps a card's body as a version when it changed since the newest one
//! ([`Store::keep_body_versions`]): which body that is, and when, is the server's to
//! say (`crates/server/src/collab`); the store tells a body that is kept already by its
//! digest, numbers the versions, and keeps the newest
//! ([`Store::set_body_versions_kept`]). A card in the trash keeps its rows; they go when
//! the card is gone for good ([`forget_gone_blocks`]). The files the kept versions show
//! are not collected ([`crate::file_refs`]).

use std::collections::BTreeSet;
use std::sync::atomic::Ordering;

use fvoci_model::{Block, Body};
use serde_json::Value;

use crate::Store;
use crate::blocks;
use crate::db::{Id, Param, Row, Scope, now_millis, to_json_text};
use crate::error::StoreError;
use crate::file_refs::{self, NewImages};

/// How many versions of a card's body the store keeps unless told otherwise.
pub const DEFAULT_BODY_VERSIONS_KEPT: u32 = 50;

/// The version of [`body_version_editors`] rows for who wrote since the newest version.
const PENDING: i64 = 0;

/// A version kept of a card's body, without the body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeptVersion {
    /// Its number among the card's, from 1; versions no longer kept leave the lowest
    /// numbers out.
    pub version: i64,
    /// When it was kept, in milliseconds since the epoch.
    pub kept_at: i64,
    /// Who wrote the body up to it since the version before, in the order of their
    /// identifiers.
    pub editors: Vec<String>,
    /// The digest of its body, as the server takes it.
    pub body_digest: Vec<u8>,
}

/// A card's body to keep as a version, if it is not the card's newest one.
#[derive(Debug, Clone, Copy)]
pub struct BodyToKeep<'a> {
    /// The card as it is stored now: its body is the version.
    pub block: &'a Block,
    /// The digest of that body.
    pub digest: &'a [u8],
    /// Who wrote it whole; empty for a body its editors typed, who are known already
    /// ([`Store::append_body_updates`]).
    pub written_by: &'a str,
}

impl Store {
    /// Sets how many versions of a card's body are kept, the newest. At least one.
    /// Counts for the versions kept after.
    pub fn set_body_versions_kept(&self, kept: u32) {
        let kept = usize::try_from(kept.max(1)).unwrap_or(usize::MAX);
        self.inner.body_versions_kept.store(kept, Ordering::Relaxed);
    }

    /// Keeps the body of each card as a version, all or none, and gives the number of
    /// each new version; `None` for a body that is the card's newest version already,
    /// and for no body of a card that has no version.
    ///
    /// A new version names who wrote the body since the one before: the users whose
    /// editors sent updates, and `written_by`. If nobody is known (a body written
    /// before versions were kept, or by an import), it names who changed the card
    /// last, when they did. Either way nobody is left as having written since.
    /// Versions past those kept go, the oldest first, and the files only they showed
    /// are shown no more.
    ///
    /// The caller holds each card against other writes of its body.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if a statement fails.
    pub async fn keep_body_versions(
        &self,
        bodies: &[BodyToKeep<'_>],
    ) -> Result<Vec<Option<i64>>, StoreError> {
        let kept = i64::try_from(self.inner.body_versions_kept.load(Ordering::Relaxed))
            .unwrap_or(i64::MAX);
        self.transaction(async |scope| {
            let mut versions = Vec::with_capacity(bodies.len());
            for body in bodies {
                versions.push(keep(scope, body, kept).await?);
            }
            Ok(versions)
        })
        .await
    }

    /// The versions kept of a card's body, the newest first, without their bodies.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if a query fails.
    pub async fn body_versions(&self, block_id: &str) -> Result<Vec<KeptVersion>, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope
                .sql("SELECT version, kept_at, body_digest FROM body_versions WHERE block_id = ");
            sql.bind(Id(block_id)).push(" ORDER BY version DESC");
            let mut versions = scope
                .fetch_all(sql)
                .await?
                .iter()
                .map(kept_version)
                .collect::<Result<Vec<_>, _>>()?;
            let mut sql =
                scope.sql("SELECT version, user_id FROM body_version_editors WHERE block_id = ");
            sql.bind(Id(block_id))
                .push_bind(" AND version > ", PENDING)
                .push(" ORDER BY user_id");
            for row in scope.fetch_all(sql).await? {
                let (version, user_id) = (row.int(0)?, row.id(1)?);
                if let Some(kept) = versions.iter_mut().find(|kept| kept.version == version) {
                    kept.editors.push(user_id);
                }
            }
            Ok(versions)
        })
        .await
    }

    /// A version kept of a card's body, with the body; `None` if the card has no such
    /// version (any more).
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if a query fails.
    pub async fn body_version(
        &self,
        block_id: &str,
        version: i64,
    ) -> Result<Option<(KeptVersion, Option<Body>)>, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql(
                "SELECT version, kept_at, body_digest, body FROM body_versions WHERE block_id = ",
            );
            sql.bind(Id(block_id)).push_bind(" AND version = ", version);
            let Some(row) = scope.fetch_optional(sql).await? else {
                return Ok(None);
            };
            let mut kept = kept_version(&row)?;
            let body = match row.json(3, "body")? {
                Value::Null => None,
                document => Some(Body::from_stored(document)),
            };
            let mut sql = scope.sql("SELECT user_id FROM body_version_editors WHERE block_id = ");
            sql.bind(Id(block_id))
                .push_bind(" AND version = ", version)
                .push(" ORDER BY user_id");
            for row in scope.fetch_all(sql).await? {
                kept.editors.push(row.id(0)?);
            }
            Ok(Some((kept, body)))
        })
        .await
    }
}

fn kept_version(row: &Row) -> Result<KeptVersion, StoreError> {
    Ok(KeptVersion {
        version: row.int(0)?,
        kept_at: row.int(1)?,
        body_digest: row.bytes(2)?,
        editors: Vec::new(),
    })
}

/// Keeps one body ([`Store::keep_body_versions`]), with at most `kept` versions left.
async fn keep(
    scope: &mut Scope<'_>,
    body: &BodyToKeep<'_>,
    kept: i64,
) -> Result<Option<i64>, StoreError> {
    let block = body.block;
    let mut sql = scope.sql("SELECT version, body_digest FROM body_versions WHERE block_id = ");
    sql.bind(Id(&block.id))
        .push(" ORDER BY version DESC LIMIT 1");
    let newest = match scope.fetch_optional(sql).await? {
        Some(row) => Some((row.int(0)?, row.bytes(1)?)),
        None => None,
    };
    let kept_already = match &newest {
        Some((_, digest)) => digest == body.digest,
        // No body is no first version: there is nothing in it to go back to.
        None => block.body.is_none(),
    };
    let mut sql = scope.sql("DELETE FROM body_version_editors WHERE block_id = ");
    sql.bind(Id(&block.id))
        .push_bind(" AND version = ", PENDING)
        .push(" RETURNING user_id");
    let mut editors: BTreeSet<String> = scope
        .fetch_all(sql)
        .await?
        .iter()
        .map(|row| row.id(0))
        .collect::<Result<_, _>>()?;
    if kept_already {
        return Ok(None);
    }
    if !body.written_by.is_empty() {
        editors.insert(body.written_by.to_owned());
    }
    let mut kept_at = now_millis();
    if editors.is_empty() && !block.modified_by.is_empty() {
        editors.insert(block.modified_by.clone());
        kept_at = block.update_at;
    }
    let version = newest.map_or(1, |(version, _)| version + 1);
    let files: Vec<&str> = block.body.as_ref().map(Body::file_ids).unwrap_or_default();
    let mut sql = scope.sql(
        "INSERT INTO body_versions (block_id, version, body, body_digest, files, kept_at) \
         VALUES (",
    );
    sql.bind(Id(&block.id))
        .push_bind(", ", version)
        .push_bind(
            ", ",
            Param::opt_jsonb(block.body.as_ref().map(Body::document))?,
        )
        .push_bind(", ", Param::Bytes(body.digest.to_vec()))
        .push_bind(", ", to_json_text(&files)?)
        .push_bind(", ", kept_at)
        .push(")");
    scope.execute(sql).await?;
    for user_id in &editors {
        let mut sql =
            scope.sql("INSERT INTO body_version_editors (block_id, version, user_id) VALUES (");
        sql.bind(Id(&block.id))
            .push_bind(", ", version)
            .push_bind(", ", Id(user_id))
            .push(")");
        scope.execute(sql).await?;
    }
    forget_past(scope, &block.id, version.saturating_sub(kept)).await?;
    Ok(Some(version))
}

/// Forgets the versions of a card up to `through`, and shows the files only they
/// showed no more.
async fn forget_past(
    scope: &mut Scope<'_>,
    block_id: &str,
    through: i64,
) -> Result<(), StoreError> {
    if through < 1 {
        return Ok(());
    }
    let mut sql = scope.sql("DELETE FROM body_versions WHERE block_id = ");
    sql.bind(Id(block_id))
        .push_bind(" AND version <= ", through);
    if scope.execute(sql).await? == 0 {
        return Ok(());
    }
    let mut sql = scope.sql("DELETE FROM body_version_editors WHERE block_id = ");
    sql.bind(Id(block_id))
        .push_bind(" AND version > ", PENDING)
        .push_bind(" AND version <= ", through);
    scope.execute(sql).await?;
    // The card as it is stored: the files it shows are put down again with those of
    // the versions that are left.
    let block = blocks::get_block(scope, block_id).await?;
    file_refs::refresh(scope, &block, NewImages::Unchecked).await
}

/// Puts down who wrote a card's body since its newest version, besides those put down
/// already.
pub(crate) async fn note_editors(
    scope: &mut Scope<'_>,
    block_id: &str,
    user_ids: &[&str],
) -> Result<(), StoreError> {
    for user_id in user_ids {
        let mut sql =
            scope.sql("INSERT INTO body_version_editors (block_id, version, user_id) VALUES (");
        sql.bind(Id(block_id))
            .push_bind(", ", PENDING)
            .push_bind(", ", Id(user_id))
            .push(") ON CONFLICT DO NOTHING");
        scope.execute(sql).await?;
    }
    Ok(())
}

/// The stored names of the files that the kept versions of a card's body show.
pub(crate) async fn files_of(
    scope: &mut Scope<'_>,
    block_id: &str,
) -> Result<Vec<String>, StoreError> {
    let mut sql = scope.sql("SELECT files FROM body_versions WHERE block_id = ");
    sql.bind(Id(block_id));
    let mut names = Vec::new();
    for row in scope.fetch_all(sql).await? {
        let files: Vec<String> =
            serde_json::from_str(&row.text(0)?).map_err(|error| StoreError::Corrupt {
                column: "files",
                reason: error.to_string(),
            })?;
        names.extend(files);
    }
    Ok(names)
}

/// Forgets the versions of the cards that are gone for good, and who wrote them:
/// neither on a board nor in the trash of the organization of the row.
pub(crate) async fn forget_gone_blocks(scope: &mut Scope<'_>) -> Result<(), StoreError> {
    for table in ["body_versions", "body_version_editors"] {
        let statement = [
            &format!(
                "DELETE FROM {table} WHERE NOT EXISTS \
                 (SELECT 1 FROM blocks b WHERE b.id = {table}.block_id"
            ),
            scope.same_organization(&[("b", table)]).as_str(),
            &format!(
                ") AND NOT EXISTS (SELECT 1 FROM deleted_blocks d WHERE d.id = {table}.block_id"
            ),
            scope.same_organization(&[("d", table)]).as_str(),
            ")",
        ]
        .concat();
        let sql = scope.sql(&statement);
        scope.execute(sql).await?;
    }
    Ok(())
}
