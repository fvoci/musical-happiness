//! The versions kept of the bodies of cards (`body_versions`), and who wrote the body
//! up to each (`body_version_editors`).
//!
//! The server keeps a card's body as a version when it changed since the newest one
//! ([`Store::keep_body_version`]): which body that is, and when, is the server's to
//! say (`crates/server/src/collab`); the store tells a body that is kept already by its
//! digest, numbers the versions, and keeps the newest
//! ([`Store::set_body_versions_kept`]). A card in the trash keeps its rows; they go when
//! the card is gone for good ([`forget_gone_blocks`]). The files the kept versions show
//! are not collected ([`crate::file_refs`]).
//!
//! A page's body is kept as a card's is: "card" here is any block whose body has a room
//! ([`BlockType::has_room`]).

use std::collections::BTreeSet;
use std::sync::atomic::Ordering;

use fvoci_model::{Block, BlockType, Body};
use serde_json::Value;

use crate::Store;
use crate::block_derived::{self, Before};
use crate::body_digest::body_digest;
use crate::db::{Id, Param, Row, Scope, VersionsKept, now_millis, to_json_text};
use crate::error::StoreError;
use crate::file_refs::NewImages;
use crate::{blocks, boards};

/// How many versions of a card's body the store keeps unless told otherwise.
pub const DEFAULT_BODY_VERSIONS_KEPT: u32 = 50;

/// The version of [`body_version_editors`] rows for who wrote since the newest version.
pub(crate) const PENDING: i64 = 0;

/// The SQL condition that `column`, the type of a block, is one whose body is kept
/// ([`BlockType::WITH_ROOM`]).
pub(crate) fn with_room_sql(column: &str) -> String {
    let types = BlockType::WITH_ROOM.map(|with_room| format!("'{}'", with_room.as_str()));
    format!("{column} IN ({})", types.join(", "))
}

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
struct BodyToKeep<'a> {
    /// The card as it is stored: its body is the version.
    block: &'a Block,
    /// Who wrote it whole; `None` for a body its editors typed, who are known already
    /// ([`Store::append_body_updates`]).
    written_by: Option<&'a str>,
}

impl Store {
    /// Sets how many versions of a card's body are kept, the newest. At least one.
    /// Counts for the versions kept after.
    pub fn set_body_versions_kept(&self, kept: u32) {
        self.inner
            .body_versions_kept
            .store(kept.max(1), Ordering::Relaxed);
    }

    /// Keeps the body a card holds as a version, and gives the number of the new
    /// version; `None` for a body that is the card's newest version already, for no
    /// body of a card that has no version, and for a card that is not there.
    ///
    /// The new version names who wrote the body since the one before: the users whose
    /// editors sent updates ([`Store::append_body_updates`]), and nobody is left as
    /// having written since. If nobody is known to have (a body from before versions
    /// were kept, or of an import), it names nobody and is as of the card's last
    /// change. Versions past those kept go, the oldest first, and the files only they
    /// showed are shown no more.
    ///
    /// The card is read with its board held, as a write of the card holds it: the
    /// body kept is one the card had, whatever is written meanwhile.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if a statement fails.
    pub async fn keep_body_version(&self, card_id: &str) -> Result<Option<i64>, StoreError> {
        self.transaction(async |scope| keep_held(scope, card_id).await)
            .await
    }

    /// How many versions are kept, for a scope that begins.
    pub(crate) fn versions_kept(&self) -> VersionsKept {
        VersionsKept {
            of_comments: u32::try_from(self.comment_versions_kept()).unwrap_or(u32::MAX),
            of_bodies: self.inner.body_versions_kept.load(Ordering::Relaxed),
        }
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

/// What becomes of the body a card holds before updates of its document are logged
/// ([`Store::append_body_updates`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyBefore {
    /// Kept as a version first, if it is not the card's newest, within the write of
    /// the log: updates that are not logged keep nothing. For a card that holds all
    /// its document took before.
    Keep,
    /// Left as it is: a version already, or less than the document took.
    Leave,
}

/// Keeps the body a card holds ([`Store::keep_body_version`]), read with its board
/// held.
pub(crate) async fn keep_held(
    scope: &mut Scope<'_>,
    card_id: &str,
) -> Result<Option<i64>, StoreError> {
    let Some(found) = blocks::find_block(scope, card_id).await? else {
        return Ok(None);
    };
    // Read again once its board is held: what puts down the files a card shows holds
    // the board, and so does what writes its body.
    if !boards::lock_board(scope, &found.board_id).await? {
        return Ok(None);
    }
    let card = match blocks::find_block(scope, card_id).await? {
        Some(block) if block.block_type.has_room() => block,
        _ => return Ok(None),
    };
    let body = BodyToKeep {
        block: &card,
        written_by: None,
    };
    let kept = keep(scope, &body).await?;
    if kept.forgot_some {
        // The files the card shows are put down again with those of the versions
        // that are left.
        block_derived::refresh(scope, &card, Before::Unchanged, NewImages::Unchecked).await?;
    }
    Ok(kept.version)
}

/// What a write that changes the body of a card is, for the versions kept of that body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BodyWrite {
    /// A body written whole by its writer: the body before and the body after are
    /// kept as versions within the write ([`record_change`]).
    Whole,
    /// The body of the card's document as its editors typed it so far
    /// ([`Store::project_body`]): no version of its own; the server keeps one when it
    /// is due ([`Store::keep_body_version`]).
    Projected,
}

/// What became of a body that was to be kept.
#[derive(Debug, Clone, Copy)]
struct Kept {
    /// The number of the new version; `None` if the body was kept already.
    version: Option<i64>,
    /// Whether versions past those kept went: the files the card shows are to be put
    /// down again.
    forgot_some: bool,
}

/// Keeps the versions a write of a card's body whole brings, within that write: the
/// body before, if it is not the card's newest version yet (by its editors, if any
/// typed it), and the body after, by `user_id`. Nothing for a write that
/// leaves the body as it was, and for a block whose body has no room. The caller puts down
/// the files the card shows after.
pub(crate) async fn record_change(
    scope: &mut Scope<'_>,
    before: &Block,
    after: &Block,
    user_id: &str,
) -> Result<(), StoreError> {
    if !after.block_type.has_room()
        || body_digest(before.body.as_ref()) == body_digest(after.body.as_ref())
    {
        return Ok(());
    }
    if before.block_type.has_room() {
        let before = BodyToKeep {
            block: before,
            written_by: None,
        };
        keep(scope, &before).await?;
    }
    let after = BodyToKeep {
        block: after,
        written_by: Some(user_id),
    };
    keep(scope, &after).await?;
    Ok(())
}

/// Keeps one body ([`Store::keep_body_version`]) and forgets the versions past those
/// kept.
async fn keep(scope: &mut Scope<'_>, body: &BodyToKeep<'_>) -> Result<Kept, StoreError> {
    let block = body.block;
    let digest = body_digest(block.body.as_ref());
    let mut sql = scope.sql("SELECT version, body_digest FROM body_versions WHERE block_id = ");
    sql.bind(Id(&block.id))
        .push(" ORDER BY version DESC LIMIT 1");
    let newest = match scope.fetch_optional(sql).await? {
        Some(row) => Some((row.int(0)?, row.bytes(1)?)),
        None => None,
    };
    let kept_already = match &newest {
        Some((_, newest)) => *newest == digest,
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
        return Ok(Kept {
            version: None,
            forgot_some: false,
        });
    }
    // A writer without an identifier (the server itself) is nobody to name.
    if let Some(written_by) = body.written_by.filter(|by| !by.is_empty()) {
        editors.insert(written_by.to_owned());
    }
    // Nobody is known to have written it: the body is as old as the card's last
    // change at the latest, and who made that change may have changed something else.
    let kept_at = if editors.is_empty() {
        block.update_at
    } else {
        now_millis()
    };
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
        .push_bind(", ", Param::Bytes(digest))
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
    let through = version.saturating_sub(scope.body_versions_kept());
    let forgot_some = forget_past(scope, &block.id, through).await?;
    Ok(Kept {
        version: Some(version),
        forgot_some,
    })
}

/// Forgets the versions of a card up to `through`; whether there were any. The files
/// only they showed are shown no more once the caller puts the card's files down again.
async fn forget_past(
    scope: &mut Scope<'_>,
    block_id: &str,
    through: i64,
) -> Result<bool, StoreError> {
    if through < 1 {
        return Ok(false);
    }
    let mut sql = scope.sql("DELETE FROM body_versions WHERE block_id = ");
    sql.bind(Id(block_id))
        .push_bind(" AND version <= ", through);
    if scope.execute(sql).await? == 0 {
        return Ok(false);
    }
    let mut sql = scope.sql("DELETE FROM body_version_editors WHERE block_id = ");
    sql.bind(Id(block_id))
        .push_bind(" AND version > ", PENDING)
        .push_bind(" AND version <= ", through);
    scope.execute(sql).await?;
    Ok(true)
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

/// Who is put down as having written a card's body since its newest version.
pub(crate) async fn unkept_editors(
    scope: &mut Scope<'_>,
    block_id: &str,
) -> Result<Vec<String>, StoreError> {
    let mut sql = scope.sql("SELECT user_id FROM body_version_editors WHERE block_id = ");
    sql.bind(Id(block_id))
        .push_bind(" AND version = ", PENDING)
        .push(" ORDER BY user_id");
    scope
        .fetch_all(sql)
        .await?
        .iter()
        .map(|row| row.id(0))
        .collect()
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
