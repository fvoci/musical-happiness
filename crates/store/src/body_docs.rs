//! The Y documents of the bodies people edit together, as they are stored: a state and
//! the updates taken after it (`body_docs`, `body_updates`).
//!
//! The server takes an update into a document and logs it here before anybody else is
//! sent it, folds the log into the state when it grows, and writes the document's body
//! to its block a little later ([`Store::project_body`]). A document is its block's:
//! it stays while the block is in the trash and goes with it when it is gone for good
//! ([`forget_gone_blocks`]). What a document's bytes are is the server's to know: the
//! store keeps them.

use fvoci_model::{BlockPatch, BlockType, Body, BodyChange};

use crate::Store;
use crate::block_derived::{self, Before};
use crate::blocks;
use crate::db::{Id, OptId, Param, Row, Scope, now_millis};
use crate::error::StoreError;
use crate::file_refs::{self, NewImages};

/// A stored document and the updates it took after its state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredDoc {
    /// The document as a version 1 update of all of it.
    pub state: Vec<u8>,
    /// The last update the state holds; 0 for none.
    pub state_seq: i64,
    /// The last update logged: the next one is numbered after it.
    pub log_end: i64,
    /// The last update whose body the block holds.
    pub projected_seq: i64,
    /// The digest of the block's body as it was when the document's body was last the
    /// same: a body that is not that any more was written without the document.
    pub body_digest: Vec<u8>,
    /// Which making of the document it is: 1 for the first, one more each time it is
    /// made again ([`Store::remake_body_doc`]).
    pub epoch: i64,
    /// The user whose editor sent the last update logged; empty for none.
    pub last_writer: String,
    /// The updates after `state_seq`, in the order they were taken.
    pub updates: Vec<LoggedUpdate>,
}

/// An update a document took.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoggedUpdate {
    /// Its number: one after the update before it.
    pub seq: i64,
    /// The update, version 1.
    pub bytes: Vec<u8>,
    /// The user whose editor sent it; empty for the server's own.
    pub written_by: String,
    /// When it was taken, in milliseconds since the epoch.
    pub written_at: i64,
}

impl Store {
    /// The board a card is on, or the wiki a page is in; `None` for no block of that
    /// identifier, and for a block whose body has no room ([`BlockType::has_room`]).
    /// Nothing else of the block is read: who asks whether somebody may come to its
    /// body is answered in the same time whatever the body is.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn board_of_card(&self, card_id: &str) -> Result<Option<String>, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql("SELECT type, board_id FROM blocks WHERE id = ");
            sql.bind(Id(card_id));
            let Some(row) = scope.fetch_optional(sql).await? else {
                return Ok(None);
            };
            let has_room = BlockType::from(row.text(0)?).has_room();
            Ok(if has_room { Some(row.id(1)?) } else { None })
        })
        .await
    }

    /// The stored document of a block, with the updates it took after its state;
    /// `None` if the block has none. Read in two statements outside a transaction: the
    /// server writes a block's document only while it holds the block's place, as the
    /// reader does (the purge of the trash aside, which removes it whole).
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if a query fails.
    pub async fn get_body_doc(&self, block_id: &str) -> Result<Option<StoredDoc>, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql(
                "SELECT state, state_seq, log_end, projected_seq, body_digest, epoch, \
                 last_writer FROM body_docs WHERE block_id = ",
            );
            sql.bind(Id(block_id));
            let Some(row) = scope.fetch_optional(sql).await? else {
                return Ok(None);
            };
            let state_seq = row.int(1)?;
            let mut sql = scope.sql(
                "SELECT seq, bytes, written_by, written_at FROM body_updates WHERE block_id = ",
            );
            sql.bind(Id(block_id))
                .push_bind(" AND seq > ", state_seq)
                .push(" ORDER BY seq");
            let updates = scope
                .fetch_all(sql)
                .await?
                .iter()
                .map(logged_update)
                .collect::<Result<_, _>>()?;
            Ok(Some(StoredDoc {
                state: row.bytes(0)?,
                state_seq,
                log_end: row.int(2)?,
                projected_seq: row.int(3)?,
                body_digest: row.bytes(4)?,
                epoch: row.int(5)?,
                last_writer: row.id(6)?,
                updates,
            }))
        })
        .await
    }

    /// Stores the first document of a block, made from its body, whose digest is
    /// `body_digest`: the block holds its body.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if the block is not there (or not in reach),
    /// and [`StoreError::Database`] if it has a document already.
    pub async fn create_body_doc(
        &self,
        block_id: &str,
        state: &[u8],
        body_digest: &[u8],
    ) -> Result<(), StoreError> {
        self.transaction(async |scope| {
            blocks::get_block(scope, block_id).await?;
            let now = now_millis();
            let mut sql = scope.sql(
                "INSERT INTO body_docs (block_id, state, state_seq, log_end, projected_seq, \
                 body_digest, epoch, create_at, update_at) VALUES (",
            );
            sql.bind(Id(block_id))
                .push_bind(", ", Param::Bytes(state.to_vec()))
                .push(", 0, 0, 0")
                .push_bind(", ", Param::Bytes(body_digest.to_vec()))
                .push(", 1")
                .push_bind(", ", now)
                .push_bind(", ", now)
                .push(")");
            scope.execute(sql).await?;
            Ok(())
        })
        .await
    }

    /// Logs updates a block's document took, in order and all or none, and gives
    /// their numbers. Each comes with the user whose editor sent it; empty for the
    /// server's own. `images` are the files the images of the document's body show
    /// with them: those the block does not show yet are asked of its board and held as
    /// a write of its body holds them ([`crate::NewImages::Recorded`]), and the block
    /// shows them from now on, so that the files are not collected before the body is
    /// written to it.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if the block, or its document, is not there,
    /// and [`StoreError::InvalidBlock`] if its board has no file of an image.
    pub async fn append_body_updates(
        &self,
        block_id: &str,
        updates: &[(&[u8], &str)],
        images: &[String],
    ) -> Result<Vec<i64>, StoreError> {
        self.transaction(async |scope| {
            if !images.is_empty() {
                file_refs::show_images(scope, block_id, images).await?;
            }
            let now = now_millis();
            let mut seqs = Vec::with_capacity(updates.len());
            for (bytes, written_by) in updates {
                let mut sql = scope.sql("UPDATE body_docs SET log_end = log_end + 1");
                if !written_by.is_empty() {
                    sql.push_bind(", last_writer = ", OptId(written_by));
                }
                sql.push_bind(", update_at = ", now)
                    .push_bind(" WHERE block_id = ", Id(block_id))
                    .push(" RETURNING log_end");
                let seq = scope
                    .fetch_optional(sql)
                    .await?
                    .ok_or_else(|| no_doc(block_id))?
                    .int(0)?;
                let mut sql = scope.sql(
                    "INSERT INTO body_updates (block_id, seq, bytes, written_by, written_at) \
                     VALUES (",
                );
                sql.bind(Id(block_id))
                    .push_bind(", ", seq)
                    .push_bind(", ", Param::Bytes(bytes.to_vec()))
                    .push_bind(", ", OptId(written_by))
                    .push_bind(", ", now)
                    .push(")");
                scope.execute(sql).await?;
                seqs.push(seq);
            }
            Ok(seqs)
        })
        .await
    }

    /// Folds the updates up to `through_seq` into a block's document: `state` is the
    /// document with them, and they are removed from the log.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if the block has no document.
    pub async fn fold_body_doc(
        &self,
        block_id: &str,
        state: &[u8],
        through_seq: i64,
    ) -> Result<(), StoreError> {
        self.transaction(async |scope| {
            let mut sql = scope.sql("UPDATE body_docs SET state = ");
            sql.bind(Param::Bytes(state.to_vec()))
                .push_bind(", state_seq = ", through_seq)
                .push_bind(", update_at = ", now_millis())
                .push_bind(" WHERE block_id = ", Id(block_id));
            if scope.execute(sql).await? == 0 {
                return Err(no_doc(block_id));
            }
            let mut sql = scope.sql("DELETE FROM body_updates WHERE block_id = ");
            sql.bind(Id(block_id))
                .push_bind(" AND seq <= ", through_seq);
            scope.execute(sql).await?;
            Ok(())
        })
        .await
    }

    /// Replaces a block's document with one made again from the body the block holds,
    /// whose digest is `body_digest`: the log is emptied, the numbers of updates go on
    /// after the last one, and its epoch, which this gives, is one more.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if the block has no document.
    pub async fn remake_body_doc(
        &self,
        block_id: &str,
        state: &[u8],
        body_digest: &[u8],
    ) -> Result<i64, StoreError> {
        self.transaction(async |scope| {
            let mut sql = scope.sql("UPDATE body_docs SET state = ");
            sql.bind(Param::Bytes(state.to_vec()))
                .push(", state_seq = log_end, projected_seq = log_end, epoch = epoch + 1")
                .push_bind(", body_digest = ", Param::Bytes(body_digest.to_vec()))
                .push_bind(", update_at = ", now_millis())
                .push_bind(" WHERE block_id = ", Id(block_id))
                .push(" RETURNING epoch");
            let epoch = scope
                .fetch_optional(sql)
                .await?
                .ok_or_else(|| no_doc(block_id))?
                .int(0)?;
            let mut sql = scope.sql("DELETE FROM body_updates WHERE block_id = ");
            sql.bind(Id(block_id));
            scope.execute(sql).await?;
            Ok(epoch)
        })
        .await
    }

    /// Writes the body of a block's document to the block, as the document was after
    /// the update `through_seq`: the block is patched by `user_id` as any change of its
    /// body is (its plain text, task counts and the files it shows with it), and the
    /// document is marked as written up to there, with `body_digest` the digest of
    /// `body`. With `body` `None` the block is not patched: its body is the document's
    /// already. `read_at` is the block's `update_at` as it was read: a block changed
    /// since is not written over. (A restore from the trash in the same millisecond as
    /// the read, and the import's own writing of bodies, which keeps `update_at`, are
    /// not seen; neither writes a block whose document is open.)
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if the block, or its document, is not there,
    /// [`StoreError::Stale`] if the block changed since `read_at`, and the errors of a
    /// patch of the block.
    pub async fn project_body(
        &self,
        block_id: &str,
        body: Option<Option<Body>>,
        read_at: i64,
        through_seq: i64,
        body_digest: &[u8],
        user_id: &str,
    ) -> Result<(), StoreError> {
        self.transaction(async |scope| {
            match body {
                // The block holds the body; what it shows is put down again all the
                // same, in place of the images the log showed meanwhile. Nothing else
                // that follows from the block changes: it is as it was stored.
                None => {
                    let block = blocks::get_block(scope, block_id).await?;
                    block_derived::refresh(scope, &block, Before::Unchanged, NewImages::Unchecked)
                        .await?;
                }
                Some(body) => {
                    let patch = BlockPatch {
                        body: body.map_or(BodyChange::Clear, BodyChange::Set),
                        read_at: Some(read_at),
                        ..BlockPatch::default()
                    };
                    blocks::patch_block(scope, block_id, &patch, user_id).await?;
                }
            }
            let mut sql = scope.sql("UPDATE body_docs SET projected_seq = ");
            sql.bind(through_seq)
                .push_bind(", body_digest = ", Param::Bytes(body_digest.to_vec()))
                .push_bind(" WHERE block_id = ", Id(block_id))
                .push_bind(" AND projected_seq <= ", through_seq);
            scope.execute(sql).await?;
            Ok(())
        })
        .await
    }
}

impl Store {
    /// Whether a block has a document whose log holds updates the block's body does not.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn has_unwritten_body(&self, block_id: &str) -> Result<bool, StoreError> {
        self.with_conn(async |scope| {
            let mut sql =
                scope.sql("SELECT 1 FROM body_docs WHERE projected_seq < log_end AND block_id = ");
            sql.bind(Id(block_id));
            Ok(scope.fetch_optional(sql).await?.is_some())
        })
        .await
    }

    /// Those of `block_ids` that have a document whose log holds updates the block's
    /// body does not: [`Self::has_unwritten_body`] for many blocks at once.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if a query fails.
    pub async fn with_unwritten_bodies(
        &self,
        block_ids: &[String],
    ) -> Result<Vec<String>, StoreError> {
        /// Identifiers bound in one query: far below the bound parameters either
        /// database takes.
        const CHUNK: usize = 500;
        self.with_conn(async |scope| {
            let mut found = Vec::new();
            for chunk in block_ids.chunks(CHUNK) {
                let mut sql = scope.sql(
                    "SELECT block_id FROM body_docs WHERE projected_seq < log_end AND block_id IN (",
                );
                sql.bind_list(chunk.iter().map(|id| Id(id))).push(")");
                for row in scope.fetch_all(sql).await? {
                    found.push(row.id(0)?);
                }
            }
            Ok(found)
        })
        .await
    }

    /// The documents whose block does not hold their body yet, with the organization
    /// of each: what a server that ended before it wrote them has to write. Blocks in
    /// the trash are left out; theirs is written when they are opened again.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn unwritten_body_docs(&self) -> Result<Vec<(String, String)>, StoreError> {
        self.with_conn(async |scope| {
            let statement = [
                "SELECT d.block_id, w.org_id FROM body_docs d \
                 JOIN blocks b ON b.id = d.block_id",
                &scope.same_organization(&[("b", "d")]),
                " JOIN boards o ON o.id = b.board_id",
                &scope.same_organization(&[("o", "b")]),
                " JOIN workspaces w ON w.id = o.workspace_id",
                &scope.same_organization(&[("w", "o")]),
                " WHERE d.projected_seq < d.log_end ORDER BY d.block_id",
            ]
            .concat();
            let sql = scope.sql(&statement);
            scope
                .fetch_all(sql)
                .await?
                .iter()
                .map(|row| Ok((row.id(0)?, row.id(1)?)))
                .collect()
        })
        .await
    }
}

fn logged_update(row: &Row) -> Result<LoggedUpdate, StoreError> {
    Ok(LoggedUpdate {
        seq: row.int(0)?,
        bytes: row.bytes(1)?,
        written_by: row.id(2)?,
        written_at: row.int(3)?,
    })
}

fn no_doc(block_id: &str) -> StoreError {
    StoreError::not_found(format_args!("the document of block ID={block_id}"))
}

/// Removes the documents whose blocks are gone for good: neither there nor in the
/// trash of their organization. Their logs go with them.
pub(crate) async fn forget_gone_blocks(scope: &mut Scope<'_>) -> Result<(), StoreError> {
    let gone = [
        " WHERE NOT EXISTS (SELECT 1 FROM blocks b WHERE b.id = body_docs.block_id",
        &scope.same_organization(&[("b", "body_docs")]),
        ") AND NOT EXISTS (SELECT 1 FROM deleted_blocks d WHERE d.id = body_docs.block_id",
        &scope.same_organization(&[("d", "body_docs")]),
        ")",
    ]
    .concat();
    let updates = format!(
        "DELETE FROM body_updates WHERE EXISTS (SELECT 1 FROM body_docs{gone} \
         AND body_docs.block_id = body_updates.block_id{})",
        scope.same_organization(&[("body_docs", "body_updates")])
    );
    let sql = scope.sql(&updates);
    scope.execute(sql).await?;
    let sql = scope.sql(&format!("DELETE FROM body_docs{gone}"));
    scope.execute(sql).await?;
    Ok(())
}
