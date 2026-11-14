//! The trash: deleting boards and blocks, and bringing them back.
//!
//! The tables of boards and blocks hold what exists. Deleting moves a row out of them
//! into `deleted_boards` or `deleted_blocks`, whole, as JSON; restoring moves it back.
//! So no query of what exists has to leave out what is deleted, and the foreign keys of
//! what exists hold without exceptions.
//!
//! Blocks that go with one deletion share a `deletion_id` and come back together: a
//! card with what is on it, a board with all its blocks. A board takes its memberships
//! and its file records into the trash and brings them back; its sharing, the places
//! users filed it under and the subscriptions to its blocks end with the deletion.
//!
//! What has been in the trash for longer than the retention period is removed for good
//! by [`Store::purge_trash`].

use std::collections::{HashMap, HashSet};

use fvoci_model::{Block, Board, BoardMember, BoardType, Body, FileInfo, new_id};
use serde::de::DeserializeOwned;

use crate::Store;
use crate::blocks::{self, Paging, parents_first};
use crate::boards;
use crate::body_docs;
use crate::body_versions;
use crate::category_boards::{self, BoardPlace};
use crate::comment_places;
use crate::comment_versions;
use crate::db::{Dialect, Id, OptId, Param, Row, Scope, Sql, now_millis};
use crate::error::StoreError;
use crate::file_info;
use crate::file_refs::{self, NewImages};
use crate::loose_objects;
use crate::members;
use crate::org_admin;
use crate::users;

/// A block in the trash.
#[derive(Debug, Clone, PartialEq)]
pub struct DeletedBlock {
    /// The block as it was, with its `delete_at` set to when it was deleted.
    pub block: Block,
    /// The identifier of the user who deleted it; empty if nobody did.
    pub deleted_by: String,
}

/// A board in the trash.
#[derive(Debug, Clone, PartialEq)]
pub struct DeletedBoard {
    /// The board as it was, with its `delete_at` set to when it was deleted.
    pub board: Board,
    /// The memberships the board had.
    pub members: Vec<BoardMember>,
    /// The identifier of the user who deleted it; empty if nobody did.
    pub deleted_by: String,
}

impl Store {
    /// Deletes a block and everything below it: moves them to the trash, as one
    /// deletion. Subscriptions to them end. Deleting a block that does not exist
    /// succeeds.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if a statement fails; nothing is deleted then.
    pub async fn delete_block(&self, block_id: &str, user_id: &str) -> Result<(), StoreError> {
        self.transaction(async |scope| delete_block(scope, block_id, user_id).await)
            .await
    }

    /// Returns a block that is in the trash.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if no block of that identifier is in the trash.
    pub async fn get_deleted_block(&self, block_id: &str) -> Result<DeletedBlock, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql("SELECT block, deleted_by FROM deleted_blocks WHERE id = ");
            sql.bind(Id(block_id));
            let row = scope
                .fetch_optional(sql)
                .await?
                .ok_or_else(block_not_in_trash)?;
            Ok(DeletedBlock {
                block: block_from_json(&row, 0)?,
                deleted_by: row.id(1)?,
            })
        })
        .await
    }

    /// Returns the blocks of the deletion that a block of the trash went with: the one
    /// that was deleted, and took the others along, first, then the others by
    /// identifier. Restoring any of them restores them all ([`Store::undelete_block`]).
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if no block of that identifier is in the trash.
    pub async fn get_deletion(&self, block_id: &str) -> Result<Vec<Block>, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql(
                "SELECT went.block FROM deleted_blocks named \
                 JOIN deleted_blocks went ON went.deletion_id = named.deletion_id \
                 WHERE named.id = ",
            );
            sql.bind(Id(block_id)).push(
                " ORDER BY CASE WHEN EXISTS (SELECT 1 FROM deleted_blocks above \
                 WHERE above.deletion_id = went.deletion_id AND above.id = went.parent_id) \
                 THEN 1 ELSE 0 END, went.id",
            );
            let rows = scope.fetch_all(sql).await?;
            if rows.is_empty() {
                return Err(block_not_in_trash());
            }
            rows.iter().map(|row| block_from_json(row, 0)).collect()
        })
        .await
    }

    /// Returns the blocks below a block, at any depth, by identifier: what deleting it
    /// takes along ([`Store::delete_block`]). Empty for a block that has nothing below it
    /// or does not exist.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn get_blocks_below(&self, block_id: &str) -> Result<Vec<Block>, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = with_tree_of(scope, block_id);
            sql.push(&format!(
                "SELECT {} FROM blocks WHERE id IN (SELECT id FROM tree) AND id <> ",
                blocks::block_read_columns()
            ));
            sql.bind(Id(block_id)).push(" ORDER BY id");
            blocks::fetch_blocks(scope, sql).await
        })
        .await
    }

    /// Returns the cards of a board that are in the trash at the top of what was deleted
    /// with them, the last deleted first and then by identifier: one page of them if
    /// `paging` says so (a page size of zero is every card). A card that went with the
    /// block it was on (a card, a view) comes back with that block and is not listed.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn get_deleted_cards(
        &self,
        board_id: &str,
        paging: Option<Paging>,
    ) -> Result<Vec<DeletedBlock>, StoreError> {
        self.with_conn(async |scope| {
            // The type is in the stored block.
            let is_card = match scope.dialect() {
                Dialect::Sqlite => "json_extract(block, '$.type') = 'card'",
                Dialect::Postgres => "block->>'type' = 'card'",
            };
            let mut sql =
                scope.sql("SELECT block, deleted_by FROM deleted_blocks WHERE board_id = ");
            sql.bind(Id(board_id)).push(&format!(
                " AND {is_card} AND NOT EXISTS (SELECT 1 FROM deleted_blocks below \
                 WHERE below.deletion_id = deleted_blocks.deletion_id \
                 AND below.id = deleted_blocks.parent_id) ORDER BY deleted_at DESC, id"
            ));
            push_paging(&mut sql, paging);
            scope
                .fetch_all(sql)
                .await?
                .iter()
                .map(|row| {
                    Ok(DeletedBlock {
                        block: block_from_json(row, 0)?,
                        deleted_by: row.id(1)?,
                    })
                })
                .collect()
        })
        .await
    }

    /// Restores a block from the trash, with every block that was deleted along with
    /// it, and returns the block. The restored blocks are marked as modified by
    /// `user_id` now.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if no block of that identifier is in the trash,
    /// or there is nothing to restore it to: its board does not exist (any more, or is
    /// in the trash itself), or the block it was on does not. Returns
    /// [`StoreError::NotRestorable`] if a block at the top would come back where it may
    /// not be: a comment off a card, or any block below a comment.
    pub async fn undelete_block(&self, block_id: &str, user_id: &str) -> Result<Block, StoreError> {
        self.transaction(async |scope| undelete_block(scope, block_id, user_id).await)
            .await
    }

    /// Deletes a board with all its blocks: moves them to the trash, together with the
    /// board's memberships, file records and the places users filed it under. The
    /// board's sharing and the subscriptions to its blocks end.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if there is no such board.
    pub async fn delete_board(&self, board_id: &str, user_id: &str) -> Result<(), StoreError> {
        self.transaction(async |scope| {
            // Before the board's: with a joining that ends (`joinings::give_back_roles`).
            org_admin::lock_organization(scope).await?;
            delete_board(scope, board_id, user_id).await
        })
        .await
    }

    /// Returns a board that is in the trash, with the memberships it had.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if no board of that identifier is in the trash.
    pub async fn get_deleted_board(&self, board_id: &str) -> Result<DeletedBoard, StoreError> {
        self.with_conn(async |scope| {
            let mut sql =
                scope.sql("SELECT board, members, deleted_by FROM deleted_boards WHERE id = ");
            sql.bind(Id(board_id));
            let row = scope
                .fetch_optional(sql)
                .await?
                .ok_or_else(board_not_in_trash)?;
            Ok(DeletedBoard {
                board: from_json(&row, 0, "board")?,
                members: from_json(&row, 1, "members")?,
                deleted_by: row.id(2)?,
            })
        })
        .await
    }

    /// Returns the boards of a workspace that are in the trash and, with an `admin_id`,
    /// that the user was an admin of when they were deleted (`schemeAdmin` of the
    /// membership they had); without one, every board of the workspace in the trash. With
    /// their memberships, the last deleted first and then by identifier: one page of them
    /// if `paging` says so (a page size of zero is every board).
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn get_deleted_boards(
        &self,
        workspace_id: &str,
        admin_id: Option<&str>,
        paging: Option<Paging>,
    ) -> Result<Vec<DeletedBoard>, StoreError> {
        self.with_conn(async |scope| {
            // The memberships are the JSON array of the board's members when it went.
            let was_admin = match scope.dialect() {
                Dialect::Sqlite => {
                    " AND EXISTS (SELECT 1 FROM json_each(deleted_boards.members) member \
                     WHERE json_extract(member.value, '$.schemeAdmin') = 1 \
                     AND json_extract(member.value, '$.userId') = "
                }
                Dialect::Postgres => {
                    " AND EXISTS (SELECT 1 FROM json_array_elements(deleted_boards.members) \
                     member WHERE member->>'schemeAdmin' = 'true' AND member->>'userId' = "
                }
            };
            let mut sql = scope
                .sql("SELECT board, members, deleted_by FROM deleted_boards WHERE workspace_id = ");
            sql.bind(Id(workspace_id));
            if let Some(admin_id) = admin_id {
                sql.push_bind(was_admin, admin_id).push(")");
            }
            sql.push(" ORDER BY deleted_at DESC, id");
            push_paging(&mut sql, paging);
            scope
                .fetch_all(sql)
                .await?
                .iter()
                .map(|row| {
                    Ok(DeletedBoard {
                        board: from_json(row, 0, "board")?,
                        members: from_json(row, 1, "members")?,
                        deleted_by: row.id(2)?,
                    })
                })
                .collect()
        })
        .await
    }

    /// Restores a board from the trash and returns it: the board, the blocks that were
    /// deleted with it, its file records, and the memberships and places in the sidebar
    /// of the users who still exist: under the category the user had filed it under, or
    /// the user's default category if that category is gone since. The board is marked
    /// as modified by `user_id` now. Its sharing does not
    /// come back: a restored board is not shared until someone shares it again.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if no board of that identifier is in the trash.
    pub async fn undelete_board(&self, board_id: &str, user_id: &str) -> Result<Board, StoreError> {
        self.transaction(async |scope| {
            // Before the board's: with a joining that ends (`joinings::give_back_roles`).
            org_admin::lock_organization(scope).await?;
            undelete_board(scope, board_id, user_id).await
        })
        .await
    }

    /// Removes for good what was deleted before `deleted_before` (milliseconds since
    /// the Unix epoch), and the deleted blocks of boards that are gone for good. Returns
    /// how many boards and blocks went, and the file records of the boards that went,
    /// whose objects are doomed in the same transaction ([`Store::due_doomed_objects`]).
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if a statement fails; nothing is removed then.
    pub async fn purge_trash(&self, deleted_before: i64) -> Result<Purged, StoreError> {
        self.transaction(async |scope| {
            let mut purged = remove_boards(scope, Trashed::Before(deleted_before)).await?;
            let mut sql = scope.sql("DELETE FROM deleted_blocks WHERE deleted_at < ");
            sql.bind(deleted_before);
            purged.records += scope.execute(sql).await?;
            purged.records += remove_what_they_left(scope).await?;
            Ok(purged)
        })
        .await
    }
}

/// Which boards of the trash [`remove_boards`] removes for good.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Trashed<'a> {
    /// Those deleted before the time, in milliseconds since the Unix epoch.
    Before(i64),
    /// Those of the workspace, which goes for good.
    OfWorkspace(&'a str),
}

/// Removes boards from the trash for good and dooms the objects of their files: the
/// one way a board and its files go. Returns how many went, and their file records.
pub(crate) async fn remove_boards(
    scope: &mut Scope<'_>,
    which: Trashed<'_>,
) -> Result<Purged, StoreError> {
    // One statement decides and deletes: a board restored meanwhile is not deleted, and
    // so its files are not doomed.
    let org = loose_objects::org_column(scope);
    let mut sql = scope.sql("DELETE FROM deleted_boards WHERE ");
    match which {
        Trashed::Before(deleted_before) => sql.push_bind("deleted_at < ", deleted_before),
        Trashed::OfWorkspace(workspace_id) => sql.push_bind("workspace_id = ", Id(workspace_id)),
    };
    sql.push(&format!(
        " RETURNING files{}",
        org.map_or(String::new(), |column| format!(", {column}"))
    ));
    let boards = scope.fetch_all(sql).await?;
    let records = u64::try_from(boards.len()).unwrap_or(u64::MAX);
    let mut files = Vec::new();
    let mut doomed = Vec::new();
    for row in &boards {
        let of_board: StoredFiles = from_json(row, 0, "files")?;
        let org_id = org.map(|_| row.id(1)).transpose()?;
        for file in Vec::<FileInfo>::from(of_board) {
            // Nothing else says where the file is once the board is gone.
            doomed.push((file.path.clone(), org_id.clone()));
            files.push(file);
        }
    }
    loose_objects::doom(scope, &doomed).await?;
    Ok(Purged { records, files })
}

/// Removes what boards that went for good leave behind: the blocks deleted on their
/// own from them, what those showed and the texts of their comments. Returns how many
/// blocks went.
pub(crate) async fn remove_what_they_left(scope: &mut Scope<'_>) -> Result<u64, StoreError> {
    // A block that was deleted on its own while its board existed has nothing to come
    // back to once the board is neither there nor in the trash of the block's
    // organization.
    let orphans = [
        "DELETE FROM deleted_blocks WHERE NOT EXISTS (SELECT 1 FROM boards b \
         WHERE b.id = deleted_blocks.board_id",
        &scope.same_organization(&[("b", "deleted_blocks")]),
        ") AND NOT EXISTS (SELECT 1 FROM deleted_boards d \
         WHERE d.id = deleted_blocks.board_id",
        &scope.same_organization(&[("d", "deleted_blocks")]),
        ")",
    ]
    .concat();
    let sql = scope.sql(&orphans);
    let records = scope.execute(sql).await?;
    // What the blocks gone for good showed is shown no more, and the texts the comments
    // among them had go with them.
    comment_versions::forget_gone_comments(scope).await?;
    body_docs::forget_gone_blocks(scope).await?;
    body_versions::forget_gone_blocks(scope).await?;
    file_refs::forget_gone_blocks(scope).await?;
    file_refs::mark(scope, None).await?;
    Ok(records)
}

/// What a purge of the trash removed for good.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Purged {
    /// How many boards and blocks went.
    pub records: u64,
    /// The file records of the boards that went. Their objects are doomed.
    pub files: Vec<FileInfo>,
}

fn block_not_in_trash() -> StoreError {
    StoreError::not_found("deleted block")
}

fn board_not_in_trash() -> StoreError {
    StoreError::not_found("deleted board")
}

/// Limits a list of the trash to one page, if `paging` has a page size.
fn push_paging(sql: &mut Sql, paging: Option<Paging>) {
    if let Some(paging) = paging.filter(|paging| paging.per_page > 0) {
        let offset = i64::from(paging.page) * i64::from(paging.per_page);
        sql.push_bind(" LIMIT ", i64::from(paging.per_page))
            .push_bind(" OFFSET ", offset);
    }
}

/// Reads a block of the trash. Its body is taken as it was stored: it was checked when
/// it was written, against the allowlist of that time, which a later one may not
/// accept.
fn block_from_json(row: &Row, index: usize) -> Result<Block, StoreError> {
    let corrupt = |error: serde_json::Error| StoreError::Corrupt {
        column: "block",
        reason: error.to_string(),
    };
    let mut value = row.json(index, "block")?;
    let body = value
        .as_object_mut()
        .and_then(|members| members.remove("body"))
        .filter(|body| !body.is_null());
    let mut block: Block = serde_json::from_value(value).map_err(corrupt)?;
    block.body = body.map(Body::from_stored);
    Ok(block)
}

/// What becomes of a user's memberships of the boards of a workspace that are in the
/// trash, when the user stops being what they were in the workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TrashedRoles {
    /// They go: for who leaves the workspace, and who comes into it anew, and is not to
    /// find the roles of an earlier time on a board that is restored.
    Forget,
    /// An admin is an editor: for who becomes a guest of the workspace.
    NoMoreThanEditor,
}

/// Holds the boards of a workspace that are in the trash, in the order of their
/// identifiers, against whoever restores one or changes who it comes back with. Who
/// also holds a membership of the workspace takes these first, as a restore does.
pub(crate) async fn hold_deleted_boards(
    scope: &mut Scope<'_>,
    workspace_id: &str,
) -> Result<(), StoreError> {
    if scope.dialect() != Dialect::Postgres {
        return Ok(());
    }
    let mut sql = scope.sql("SELECT id FROM deleted_boards WHERE workspace_id = ");
    sql.bind(Id(workspace_id)).push(" ORDER BY id FOR UPDATE");
    scope.fetch_all(sql).await?;
    Ok(())
}

/// Changes the memberships of a user that the boards of a workspace in the trash would
/// come back with, as `roles` says. The boards are held ([`hold_deleted_boards`]) from
/// before they are read: what is written back is the whole list of a board's members.
pub(crate) async fn change_member_of_deleted_boards(
    scope: &mut Scope<'_>,
    workspace_id: &str,
    user_id: &str,
    roles: TrashedRoles,
) -> Result<(), StoreError> {
    hold_deleted_boards(scope, workspace_id).await?;
    let mut sql = scope.sql("SELECT id, members FROM deleted_boards WHERE workspace_id = ");
    sql.bind(Id(workspace_id));
    for row in scope.fetch_all(sql).await? {
        let members: Vec<BoardMember> = from_json(&row, 1, "members")?;
        let changes = |member: &BoardMember| {
            member.user_id == user_id && (roles == TrashedRoles::Forget || member.scheme_admin)
        };
        if !members.iter().any(changes) {
            continue;
        }
        let kept: Vec<BoardMember> = members
            .into_iter()
            .filter_map(|member| match roles {
                _ if !changes(&member) => Some(member),
                TrashedRoles::Forget => None,
                TrashedRoles::NoMoreThanEditor => {
                    Some(member.with_role(Some(fvoci_model::BoardRole::Editor)))
                }
            })
            .collect();
        let mut sql = scope.sql("UPDATE deleted_boards SET members = ");
        sql.bind(Param::json(&kept)?)
            .push_bind(" WHERE id = ", Id(&row.id(0)?));
        scope.execute(sql).await?;
    }
    Ok(())
}

/// Reads a JSON column of the trash into the type it was written from.
pub(crate) fn from_json<T: DeserializeOwned>(
    row: &Row,
    index: usize,
    column: &'static str,
) -> Result<T, StoreError> {
    serde_json::from_value(row.json(index, column)?).map_err(|error| StoreError::Corrupt {
        column,
        reason: error.to_string(),
    })
}

/// Starts a statement over a block and everything below it: a `WITH` clause that
/// defines `tree`, the identifiers of those blocks.
///
/// `UNION` and not `UNION ALL`: it also ends the recursion if parents ever form a cycle.
fn with_tree_of(scope: &Scope<'_>, block_id: &str) -> crate::db::Sql {
    let mut sql = scope.sql("WITH RECURSIVE tree(id) AS (SELECT id FROM blocks WHERE id = ");
    sql.bind(Id(block_id))
        .push(" UNION SELECT b.id FROM blocks AS b JOIN tree ON b.parent_id = tree.id) ");
    sql
}

pub(crate) async fn delete_block(
    scope: &mut Scope<'_>,
    block_id: &str,
    user_id: &str,
) -> Result<(), StoreError> {
    // The block says which board it is on, and the board is then held: what is below
    // the block is read, written to the trash and deleted without anything being added
    // or taken away in between. Read again under the lock, the block may be gone.
    let Some(block) = blocks::find_block(scope, block_id).await? else {
        tracing::debug!(block_id, "delete_block: block not found");
        return Ok(());
    };
    boards::lock_board(scope, &block.board_id).await?;
    let mut sql = with_tree_of(scope, block_id);
    sql.push(&format!(
        "SELECT {} FROM blocks WHERE id IN (SELECT id FROM tree)",
        blocks::block_read_columns()
    ));
    let doomed = blocks::fetch_blocks(scope, sql).await?;
    if doomed.is_empty() {
        tracing::debug!(block_id, "delete_block: block deleted meanwhile");
        return Ok(());
    }

    put_blocks_in_trash(scope, &doomed, &new_id(), now_millis(), user_id).await?;

    // One statement for parents and children: the reference from a block to its parent
    // is checked when the statement is done.
    let mut sql = with_tree_of(scope, block_id);
    sql.push("DELETE FROM blocks WHERE id IN (SELECT id FROM tree)");
    scope.execute(sql).await?;
    Ok(())
}

/// Writes the blocks into the trash as one deletion.
async fn put_blocks_in_trash(
    scope: &mut Scope<'_>,
    blocks: &[Block],
    deletion_id: &str,
    deleted_at: i64,
    deleted_by: &str,
) -> Result<(), StoreError> {
    for block in blocks {
        let in_trash = Block {
            delete_at: deleted_at,
            ..block.clone()
        };
        let mut sql = scope.sql(
            "INSERT INTO deleted_blocks \
             (id, board_id, parent_id, deleted_at, deleted_by, deletion_id, block) VALUES (",
        );
        sql.bind(Id(&block.id))
            .push_bind(", ", Id(&block.board_id))
            .push_bind(", ", OptId(&block.parent_id))
            .push_bind(", ", deleted_at)
            .push_bind(", ", OptId(deleted_by))
            .push_bind(", ", Id(deletion_id))
            .push_bind(", ", Param::json(&in_trash)?)
            .push(")");
        scope.execute(sql).await?;
    }
    Ok(())
}

/// Takes the blocks of one deletion out of the trash and puts them back among the
/// blocks, marked as modified by `user_id` at `now`. Returns them.
async fn take_blocks_out_of_trash(
    scope: &mut Scope<'_>,
    deletion_id: &str,
    user_id: &str,
    now: i64,
) -> Result<Vec<Block>, StoreError> {
    let mut sql = scope.sql("SELECT block FROM deleted_blocks WHERE deletion_id = ");
    sql.bind(Id(deletion_id));
    let mut restored: Vec<Block> = scope
        .fetch_all(sql)
        .await?
        .iter()
        .map(|row| block_from_json(row, 0))
        .collect::<Result<_, _>>()?;
    let mut known_users = KnownUsers::default();
    for block in &mut restored {
        block.delete_at = 0;
        block.update_at = now;
        user_id.clone_into(&mut block.modified_by);
        known_users
            .forget_if_gone(scope, &mut block.created_by)
            .await?;
    }
    // A statement per block: each is checked on its own, so a parent goes first. What
    // they show, the trash kept for them.
    for index in parents_first(&restored) {
        blocks::insert_block_row(scope, &restored[index], NewImages::Unchecked).await?;
    }
    let mut sql = scope.sql("DELETE FROM deleted_blocks WHERE deletion_id = ");
    sql.bind(Id(deletion_id));
    scope.execute(sql).await?;
    Ok(restored)
}

pub(crate) async fn undelete_block(
    scope: &mut Scope<'_>,
    block_id: &str,
    user_id: &str,
) -> Result<Block, StoreError> {
    // The trash says which board the block was on. The board must be there to come back
    // to, and is then held: read again under the lock, the block may be restored already.
    let mut sql = scope.sql("SELECT board_id FROM deleted_blocks WHERE id = ");
    sql.bind(Id(block_id));
    let row = scope
        .fetch_optional(sql)
        .await?
        .ok_or_else(block_not_in_trash)?;
    if !boards::lock_board(scope, &row.id(0)?).await? {
        return Err(block_not_in_trash());
    }
    let mut sql = scope.sql("SELECT deletion_id FROM deleted_blocks WHERE id = ");
    sql.bind(Id(block_id));
    let row = scope
        .fetch_optional(sql)
        .await?
        .ok_or_else(block_not_in_trash)?;
    let deletion_id = row.id(0)?;

    // For the blocks at the top of what was deleted, the block they were on must be
    // there too, and still one they may be on.
    let mut sql = scope.sql("SELECT block FROM deleted_blocks WHERE deletion_id = ");
    sql.bind(Id(&deletion_id));
    let family: Vec<Block> = scope
        .fetch_all(sql)
        .await?
        .iter()
        .map(|row| block_from_json(row, 0))
        .collect::<Result<_, StoreError>>()?;
    let coming_back: HashSet<&str> = family.iter().map(|block| block.id.as_str()).collect();
    for block in &family {
        if coming_back.contains(block.parent_id.as_str()) {
            continue;
        }
        let parent = if block.parent_id.is_empty() {
            None
        } else {
            let Some(parent) = blocks::find_block(scope, &block.parent_id).await? else {
                return Err(block_not_in_trash());
            };
            Some(parent)
        };
        let parent_type = parent.as_ref().map(|parent| &parent.block_type);
        if let Some(source) = comment_places::restore_refusal(&block.block_type, parent_type) {
            return Err(StoreError::NotRestorable {
                id: block.id.clone(),
                source,
            });
        }
    }

    let restored = take_blocks_out_of_trash(scope, &deletion_id, user_id, now_millis()).await?;
    restored
        .into_iter()
        .find(|block| block.id == block_id)
        .ok_or_else(block_not_in_trash)
}

pub(crate) async fn delete_board(
    scope: &mut Scope<'_>,
    board_id: &str,
    user_id: &str,
) -> Result<(), StoreError> {
    // Held from before anything of the board is read: nothing is added to the board, or
    // stored for it, between what goes to the trash and what is deleted.
    boards::lock_board_for_deletion(scope, board_id).await?;
    let board = boards::get_board(scope, board_id).await?;
    let members = boards::get_members_for_board(scope, board_id).await?;
    let files = file_info::file_infos_of_board(scope, board_id).await?;
    let board_blocks = blocks::get_blocks(scope, &blocks::BlockFilter::board(board_id)).await?;
    // Where users filed the board, to file it there again when it comes back.
    let places: Vec<StoredPlace> = category_boards::places_of_board(scope, board_id)
        .await?
        .into_iter()
        .map(StoredPlace::from)
        .collect();

    let now = now_millis();
    let deletion_id = new_id();
    put_blocks_in_trash(scope, &board_blocks, &deletion_id, now, user_id).await?;
    let in_trash = Board {
        delete_at: now,
        ..board
    };
    let mut sql = scope.sql(
        "INSERT INTO deleted_boards \
         (id, workspace_id, deleted_at, deleted_by, deletion_id, board, members, files, places) \
         VALUES (",
    );
    sql.bind(Id(board_id))
        .push_bind(", ", Id(&in_trash.workspace_id))
        .push_bind(", ", now)
        .push_bind(", ", OptId(user_id))
        .push_bind(", ", Id(&deletion_id))
        .push_bind(", ", Param::json(&in_trash)?)
        .push_bind(", ", Param::json(&members)?)
        .push_bind(", ", Param::json(&StoredFiles::from(files))?)
        .push_bind(", ", Param::json(&places)?)
        .push(")");
    scope.execute(sql).await?;

    crate::board_team_trash::keep(scope, board_id).await?;

    // The blocks first: a board does not go while it has any. With the board go its
    // memberships, its sharing, its file records and where users filed it (kept above).
    let mut sql = scope.sql("DELETE FROM blocks WHERE board_id = ");
    sql.bind(Id(board_id));
    scope.execute(sql).await?;
    let mut sql = scope.sql("DELETE FROM boards WHERE id = ");
    sql.bind(Id(board_id));
    scope.execute(sql).await?;
    Ok(())
}

pub(crate) async fn undelete_board(
    scope: &mut Scope<'_>,
    board_id: &str,
    user_id: &str,
) -> Result<Board, StoreError> {
    let mut sql = scope
        .sql("SELECT board, members, files, deletion_id, places FROM deleted_boards WHERE id = ");
    sql.bind(Id(board_id));
    // Of two requests that restore the board at once, the second waits here and then
    // finds the trash without it.
    if scope.dialect() == Dialect::Postgres {
        sql.push(" FOR UPDATE");
    }
    let row = scope
        .fetch_optional(sql)
        .await?
        .ok_or_else(board_not_in_trash)?;
    let mut board: Board = from_json(&row, 0, "board")?;
    let members: Vec<BoardMember> = from_json(&row, 1, "members")?;
    let files: StoredFiles = from_json(&row, 2, "files")?;
    let deletion_id = row.id(3)?;
    let places: Vec<StoredPlace> = from_json(&row, 4, "places")?;

    let now = now_millis();
    board.delete_at = 0;
    board.update_at = now;
    user_id.clone_into(&mut board.modified_by);
    let mut known_users = KnownUsers::default();
    known_users
        .forget_if_gone(scope, &mut board.created_by)
        .await?;
    boards::insert_board_row(scope, &board).await?;

    for member in &members {
        // A membership of a user whose row is gone since does not come back.
        if known_users.has(scope, &member.user_id).await? {
            boards::save_member(scope, member).await?;
        }
    }
    // A board does not come back without an admin who can act as one: where those it
    // had are gone from its workspace since, or are its guests now, who brings it back
    // is its admin.
    if !user_id.is_empty()
        && members::has_no_admin(scope, board_id).await?
        && members::works_in(scope, &board.workspace_id, user_id).await?
    {
        let admin = BoardMember {
            board_id: board_id.to_owned(),
            user_id: user_id.to_owned(),
            ..BoardMember::default()
        }
        .with_role(Some(fvoci_model::BoardRole::Admin));
        boards::save_member(scope, &admin).await?;
    }
    crate::board_team_trash::restore(scope, &board, user_id).await?;
    // Each user who still exists finds the board where they had filed it; a private
    // board, only those who are its members again (a member removed earlier keeps the
    // place but cannot see the board).
    for place in places.into_iter().map(BoardPlace::from) {
        let sees = board.board_type == BoardType::Open
            || members.iter().any(|member| member.user_id == place.user_id)
            || crate::board_team_access::role_for(scope, board_id, &place.user_id)
                .await?
                .is_some();
        if sees && known_users.has(scope, &place.user_id).await? {
            category_boards::put_back(scope, board_id, &board.workspace_id, &place).await?;
        }
    }
    for file in Vec::<FileInfo>::from(files) {
        file_info::save_file_info(scope, &file).await?;
    }
    take_blocks_out_of_trash(scope, &deletion_id, user_id, now).await?;
    // The trash kept no marks: a file that no block of the board shows is counted from
    // now, as at a file's upload, and one that a block shows has none.
    let mut sql = scope.sql("UPDATE file_info SET unreferenced_at = NULL WHERE board_id = ");
    sql.bind(Id(board_id));
    scope.execute(sql).await?;
    let mut sql = scope.sql("UPDATE file_info SET unreferenced_at = ");
    sql.bind(now)
        .push(" WHERE NOT EXISTS (SELECT 1 FROM file_refs r WHERE r.board_id = file_info.board_id AND r.name = file_info.name) AND board_id = ")
        .bind(Id(board_id));
    scope.execute(sql).await?;

    let mut sql = scope.sql("DELETE FROM deleted_boards WHERE id = ");
    sql.bind(Id(board_id));
    scope.execute(sql).await?;
    Ok(board)
}

/// Which users still have a row, asked once per user.
///
/// What is in the trash refers to nothing, so a user's row can be removed while a board
/// or block that names the user is there. What comes back cannot name such a user.
#[derive(Debug, Default)]
struct KnownUsers(HashMap<String, bool>);

impl KnownUsers {
    async fn has(&mut self, scope: &mut Scope<'_>, user_id: &str) -> Result<bool, StoreError> {
        if let Some(known) = self.0.get(user_id) {
            return Ok(*known);
        }
        let known = users::user_row_exists(scope, user_id).await?;
        self.0.insert(user_id.to_owned(), known);
        Ok(known)
    }

    /// Clears `user_id` if it names a user whose row is gone: made by nobody we know.
    async fn forget_if_gone(
        &mut self,
        scope: &mut Scope<'_>,
        user_id: &mut String,
    ) -> Result<(), StoreError> {
        if !user_id.is_empty() && !self.has(scope, user_id).await? {
            user_id.clear();
        }
        Ok(())
    }
}

/// A [`BoardPlace`] as the trash keeps it, in `deleted_boards.places`. Its own type, so
/// that what is stored does not change with the store's interface.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredPlace {
    user_id: String,
    category_id: String,
    sort_order: i64,
    hidden: bool,
}

impl From<BoardPlace> for StoredPlace {
    fn from(place: BoardPlace) -> Self {
        Self {
            user_id: place.user_id,
            category_id: place.category_id,
            sort_order: place.sort_order,
            hidden: place.hidden,
        }
    }
}

impl From<StoredPlace> for BoardPlace {
    fn from(place: StoredPlace) -> Self {
        Self {
            user_id: place.user_id,
            category_id: place.category_id,
            sort_order: place.sort_order,
            hidden: place.hidden,
        }
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
/// The file records of a board as they are kept with the board in the trash. A file
/// record's own JSON form is the one for clients, which leaves out where the file is.
struct StoredFiles(Vec<StoredFile>);

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct StoredFile {
    id: String,
    board_id: String,
    stored_name: String,
    name: String,
    extension: String,
    size: i64,
    path: String,
    create_at: i64,
}

impl From<Vec<FileInfo>> for StoredFiles {
    fn from(files: Vec<FileInfo>) -> Self {
        Self(
            files
                .into_iter()
                .map(|file| StoredFile {
                    id: file.id,
                    board_id: file.board_id,
                    stored_name: file.stored_name,
                    name: file.name,
                    extension: file.extension,
                    size: file.size,
                    path: file.path,
                    create_at: file.create_at,
                })
                .collect(),
        )
    }
}

impl From<StoredFiles> for Vec<FileInfo> {
    fn from(files: StoredFiles) -> Self {
        files
            .0
            .into_iter()
            .map(|file| FileInfo {
                id: file.id,
                board_id: file.board_id,
                stored_name: file.stored_name,
                name: file.name,
                extension: file.extension,
                size: file.size,
                path: file.path,
                create_at: file.create_at,
            })
            .collect()
    }
}
