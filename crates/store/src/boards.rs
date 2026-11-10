//! Boards, their history, board membership and board templates.
//!
//! Every write of a board also appends a row to the board history. Deleting a board
//! removes it and its blocks from the main tables and appends history rows with
//! `delete_at` set; undeleting restores the newest history rows. Memberships are kept in
//! a table of their own, with a history of who joined and left.

use std::fmt::Write as _;

use fvoci_model::{
    Board, BoardError, BoardMember, BoardPatch, BoardRole, BoardSearchField, BoardType,
    GLOBAL_TEAM_ID, JsonMap, SYSTEM_USER_ID,
};
use md5::{Digest as _, Md5};
use serde_json::Value;

use crate::Store;
use crate::blocks::{self, HistoryOrder, HistoryQuery};
use crate::db::{Dialect, Param, Row, Scope, Sql, now_millis};
use crate::error::StoreError;
use crate::subscriptions;

/// The name of every board column, in the order rows are written.
const BOARD_COLUMNS: &str = "id, team_id, channel_id, created_by, modified_by, type, \
                             minimum_role, title, description, icon, show_description, \
                             is_template, template_version, properties, card_properties, \
                             create_at, update_at, delete_at";

/// Whether a board query also returns the open boards the user is not a member of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicBoards {
    /// Open boards are returned whether or not the user is a member.
    Include,
    /// Only boards the user is a member of are returned.
    Exclude,
}

/// One entry of the membership history of a board: a user joined or left.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardMemberHistoryEntry {
    /// The identifier of the board.
    pub board_id: String,
    /// The identifier of the user.
    pub user_id: String,
    /// What happened: `created` or `deleted`.
    pub action: String,
    /// When it happened, in milliseconds since the Unix epoch.
    pub insert_at: i64,
}

impl Store {
    /// Inserts `board`, or updates the board that already has its identifier, and records
    /// the new state in the history.
    ///
    /// `board` is updated to what was stored: `user_id` as modifier and the current time
    /// as modification time and, for a new board, `user_id` as creator and the current
    /// time as creation time. An update leaves the stored team, creator and creation time
    /// alone, and does not copy them into `board`. A template of the global team also
    /// gets a `trackingTemplateId` property derived from its title.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if a statement fails.
    pub async fn insert_board(&self, board: &mut Board, user_id: &str) -> Result<(), StoreError> {
        self.transaction(async |scope| insert_board(scope, board, user_id).await)
            .await
    }

    /// Inserts or updates `board` as [`Store::insert_board`] does and makes its creator
    /// an administrator and editor of it. Returns that membership.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if a statement fails.
    pub async fn insert_board_with_admin(
        &self,
        board: &mut Board,
        user_id: &str,
    ) -> Result<BoardMember, StoreError> {
        self.transaction(async |scope| insert_board_with_admin(scope, board, user_id).await)
            .await
    }

    /// Applies `patch` to the board with the given identifier and returns the patched
    /// board.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if there is no such board.
    pub async fn patch_board(
        &self,
        board_id: &str,
        patch: &BoardPatch,
        user_id: &str,
    ) -> Result<Board, StoreError> {
        self.transaction(async |scope| patch_board(scope, board_id, patch, user_id).await)
            .await
    }

    /// Returns the board with the given identifier.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if there is no such board, and
    /// [`StoreError::Corrupt`] if its stored type or minimum role is not a known one.
    pub async fn get_board(&self, board_id: &str) -> Result<Board, StoreError> {
        self.with_conn(async |scope| get_board(scope, board_id).await)
            .await
    }

    /// Returns the boards of a team that a user is a member of and, with
    /// [`PublicBoards::Include`], the open boards of the team, in no particular order.
    /// Templates are left out.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn get_boards_for_user_and_team(
        &self,
        user_id: &str,
        team_id: &str,
        public_boards: PublicBoards,
    ) -> Result<Vec<Board>, StoreError> {
        self.with_conn(async |scope| {
            get_boards_for_user_and_team(scope, user_id, team_id, public_boards).await
        })
        .await
    }

    /// Returns the boards of a team with the given identifiers, in no particular order.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotAllFound`] unless exactly one board is found per
    /// identifier.
    pub async fn get_boards_in_team_by_ids(
        &self,
        board_ids: &[String],
        team_id: &str,
    ) -> Result<Vec<Board>, StoreError> {
        self.with_conn(async |scope| get_boards_in_team_by_ids(scope, board_ids, team_id).await)
            .await
    }

    /// Deletes a board and all its blocks, recording the deletions in the history.
    /// Memberships of the board are kept.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if there is no such board.
    pub async fn delete_board(&self, board_id: &str, user_id: &str) -> Result<(), StoreError> {
        self.transaction(async |scope| delete_board(scope, board_id, user_id).await)
            .await
    }

    /// Restores a deleted board and its deleted blocks from the history. Undeleting a
    /// board that never existed, or that is not deleted, succeeds.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if a statement fails.
    pub async fn undelete_board(
        &self,
        board_id: &str,
        modified_by: &str,
    ) -> Result<(), StoreError> {
        self.transaction(async |scope| undelete_board(scope, board_id, modified_by).await)
            .await
    }

    /// Returns the number of boards that are neither templates nor marked as deleted.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn get_board_count(&self) -> Result<i64, StoreError> {
        self.with_conn(get_board_count).await
    }

    /// Returns the recorded states of a board.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn get_board_history(
        &self,
        board_id: &str,
        query: &HistoryQuery,
    ) -> Result<Vec<Board>, StoreError> {
        self.with_conn(async |scope| get_board_history(scope, board_id, query).await)
            .await
    }

    /// Makes a user a member of a board with the scheme flags of `member`, or replaces
    /// the scheme flags of an existing membership. A new membership is recorded in the
    /// membership history.
    ///
    /// The `roles`, `minimum_role` and `synthetic` members of `member` are not stored.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if a statement fails.
    pub async fn save_member(&self, member: &BoardMember) -> Result<(), StoreError> {
        self.transaction(async |scope| save_member(scope, member).await)
            .await
    }

    /// Removes a user from a board, records that in the membership history, and ends the
    /// user's subscriptions to the blocks of the board, deleted blocks included. Removing
    /// a user who is not a member succeeds and changes nothing.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if a statement fails.
    pub async fn delete_member(&self, board_id: &str, user_id: &str) -> Result<(), StoreError> {
        self.transaction(async |scope| delete_member(scope, board_id, user_id).await)
            .await
    }

    /// Returns a user's membership of a board.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if the user is not a member of the board.
    pub async fn get_member_for_board(
        &self,
        board_id: &str,
        user_id: &str,
    ) -> Result<BoardMember, StoreError> {
        self.with_conn(async |scope| get_member_for_board(scope, board_id, user_id).await)
            .await
    }

    /// Returns when a user joined and left a board, newest first, at most `limit`
    /// entries.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn get_board_member_history(
        &self,
        board_id: &str,
        user_id: &str,
        limit: Option<u32>,
    ) -> Result<Vec<BoardMemberHistoryEntry>, StoreError> {
        self.with_conn(async |scope| {
            get_board_member_history(scope, board_id, user_id, limit).await
        })
        .await
    }

    /// Returns the members of a board, in no particular order.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn get_members_for_board(
        &self,
        board_id: &str,
    ) -> Result<Vec<BoardMember>, StoreError> {
        self.with_conn(async |scope| get_members_for_board(scope, board_id).await)
            .await
    }

    /// Returns a user's memberships, in no particular order.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn get_members_for_user(
        &self,
        user_id: &str,
    ) -> Result<Vec<BoardMember>, StoreError> {
        self.with_conn(async |scope| get_members_for_user(scope, user_id).await)
            .await
    }

    /// Returns the boards, of any team, that match a search term and that a user is a
    /// member of or, with [`PublicBoards::Include`], that are open, in no particular
    /// order. Templates are left out.
    ///
    /// An empty `term` matches every board. Searching by title matches boards whose
    /// title contains every space-separated word of the term, ignoring case; searching
    /// by property name matches boards that have a property of that name.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn search_boards_for_user(
        &self,
        term: &str,
        search_field: BoardSearchField,
        user_id: &str,
        public_boards: PublicBoards,
    ) -> Result<Vec<Board>, StoreError> {
        self.with_conn(async |scope| {
            search_boards_for_user(scope, term, search_field, user_id, public_boards).await
        })
        .await
    }

    /// Returns the boards of a team whose title matches a search term and that are open
    /// or private with the user as a member, in no particular order. Templates are left
    /// out.
    ///
    /// An empty `term` matches every board; otherwise the title must contain every
    /// space-separated word of the term, ignoring case.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn search_boards_for_user_in_team(
        &self,
        team_id: &str,
        term: &str,
        user_id: &str,
    ) -> Result<Vec<Board>, StoreError> {
        self.with_conn(async |scope| {
            search_boards_for_user_in_team(scope, team_id, term, user_id).await
        })
        .await
    }

    /// Returns the templates of a team that are open or that the user is a member of, in
    /// no particular order.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn get_template_boards(
        &self,
        team_id: &str,
        user_id: &str,
    ) -> Result<Vec<Board>, StoreError> {
        self.with_conn(async |scope| get_template_boards(scope, team_id, user_id).await)
            .await
    }

    /// Removes those of `boards` that the system user created, together with their
    /// blocks, without recording anything in the history.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if a statement fails; nothing is removed then.
    pub async fn remove_default_templates(&self, boards: &[Board]) -> Result<(), StoreError> {
        self.transaction(async |scope| remove_default_templates(scope, boards).await)
            .await
    }
}

/// The select list of a board, in the order [`board_from_row`] reads it. `alias` is
/// empty or a table alias followed by a dot.
fn board_select_list(alias: &str) -> String {
    format!(
        "{alias}id, {alias}team_id, {alias}channel_id, {alias}created_by, {alias}modified_by, \
         {alias}type, {alias}minimum_role, {alias}title, {alias}description, {alias}icon, \
         {alias}show_description, {alias}is_template, CAST({alias}template_version AS BIGINT), \
         {alias}properties, {alias}card_properties, {alias}create_at, {alias}update_at, \
         {alias}delete_at"
    )
}

/// Starts `SELECT <board columns> FROM <table>`.
fn select_boards(scope: &Scope<'_>, table: &str) -> Sql {
    scope.sql(&format!(
        "SELECT {} FROM {}",
        board_select_list(""),
        scope.table(table)
    ))
}

async fn fetch_boards(scope: &mut Scope<'_>, sql: Sql) -> Result<Vec<Board>, StoreError> {
    scope
        .fetch_all(sql)
        .await?
        .iter()
        .map(board_from_row)
        .collect()
}

fn board_from_row(row: &Row) -> Result<Board, StoreError> {
    Ok(Board {
        id: row.text(0)?,
        team_id: row.text(1)?,
        channel_id: row.text(2)?,
        created_by: row.text(3)?,
        modified_by: row.text(4)?,
        board_type: board_type_from_text(&row.text(5)?)?,
        minimum_role: board_role_from_text(&row.text(6)?)?,
        title: row.text(7)?,
        description: row.text(8)?,
        icon: row.text(9)?,
        show_description: row.bool(10)?,
        is_template: row.bool(11)?,
        template_version: row.int(12)?,
        properties: row.json_map(13, "properties")?,
        card_properties: card_properties_from_json(row.json(14, "card_properties")?)?,
        create_at: row.int(15)?,
        update_at: row.int(16)?,
        delete_at: row.int(17)?,
    })
}

fn board_type_from_text(text: &str) -> Result<BoardType, StoreError> {
    [BoardType::Open, BoardType::Private]
        .into_iter()
        .find(|board_type| board_type.as_str() == text)
        .ok_or_else(|| StoreError::Corrupt {
            column: "type",
            reason: format!("unknown board type {text:?}"),
        })
}

fn board_role_from_text(text: &str) -> Result<BoardRole, StoreError> {
    [
        BoardRole::None,
        BoardRole::Viewer,
        BoardRole::Commenter,
        BoardRole::Editor,
        BoardRole::Admin,
    ]
    .into_iter()
    .find(|role| role.as_str() == text)
    .ok_or_else(|| StoreError::Corrupt {
        column: "minimum_role",
        reason: format!("unknown board role {text:?}"),
    })
}

/// Reads the card property definitions of a board. JSON `null` reads as no definitions,
/// and a `null` definition as an empty one.
fn card_properties_from_json(value: Value) -> Result<Vec<JsonMap>, StoreError> {
    let definitions: Option<Vec<Option<JsonMap>>> =
        serde_json::from_value(value).map_err(|error| StoreError::Corrupt {
            column: "card_properties",
            reason: error.to_string(),
        })?;
    Ok(definitions
        .unwrap_or_default()
        .into_iter()
        .map(Option::unwrap_or_default)
        .collect())
}

async fn find_board(scope: &mut Scope<'_>, board_id: &str) -> Result<Option<Board>, StoreError> {
    let mut sql = select_boards(scope, "boards");
    sql.push_bind(" WHERE id = ", board_id);
    Ok(fetch_boards(scope, sql).await?.into_iter().next())
}

pub(crate) async fn get_board(scope: &mut Scope<'_>, board_id: &str) -> Result<Board, StoreError> {
    // Preserved quirk: the error names the table rather than the board that is missing
    // (`server/services/store/sqlstore/board.go:227`).
    find_board(scope, board_id)
        .await?
        .ok_or_else(|| StoreError::not_found("boards"))
}

/// Starts the query for the boards a user can reach: `SELECT DISTINCT` over boards joined
/// with their memberships, without templates, narrowed to the user's boards and, if
/// asked for, the open ones. More `AND` conditions on `b` may follow.
fn select_boards_of_user(scope: &Scope<'_>, user_id: &str, public_boards: PublicBoards) -> Sql {
    let mut sql = scope.sql(&format!(
        "SELECT DISTINCT {} FROM {} AS b LEFT JOIN {} AS bm ON b.id = bm.board_id \
         WHERE b.is_template = ",
        board_select_list("b."),
        scope.table("boards"),
        scope.table("board_members")
    ));
    sql.bind(false);
    match public_boards {
        PublicBoards::Include => sql
            .push_bind(" AND (b.type = ", BoardType::Open.as_str())
            .push_bind(" OR bm.user_id = ", user_id)
            .push(")"),
        PublicBoards::Exclude => sql.push_bind(" AND bm.user_id = ", user_id),
    };
    sql
}

pub(crate) async fn get_boards_for_user_and_team(
    scope: &mut Scope<'_>,
    user_id: &str,
    team_id: &str,
    public_boards: PublicBoards,
) -> Result<Vec<Board>, StoreError> {
    let mut sql = select_boards_of_user(scope, user_id, public_boards);
    sql.push_bind(" AND b.team_id = ", team_id);
    fetch_boards(scope, sql).await
}

pub(crate) async fn get_boards_in_team_by_ids(
    scope: &mut Scope<'_>,
    board_ids: &[String],
    team_id: &str,
) -> Result<Vec<Board>, StoreError> {
    let mut sql = select_boards(scope, "boards");
    sql.push_bind(" WHERE team_id = ", team_id)
        .push(" AND id IN (")
        .bind_list(board_ids)
        .push(")");
    let boards = fetch_boards(scope, sql).await?;
    if boards.len() == board_ids.len() {
        Ok(boards)
    } else {
        tracing::warn!(
            found = boards.len(),
            asked = board_ids.len(),
            "get_boards_in_team_by_ids: mismatched number of boards found"
        );
        Err(StoreError::NotAllFound {
            entity: "board",
            ids: board_ids.to_vec(),
        })
    }
}

pub(crate) async fn insert_board(
    scope: &mut Scope<'_>,
    board: &mut Board,
    user_id: &str,
) -> Result<(), StoreError> {
    // Every way a board is stored comes through here: created, patched, duplicated,
    // imported.
    board.validate_card_properties().map_err(BoardError::from)?;
    // The built-in templates are told apart by a hash of their title.
    if board.is_template && board.team_id == GLOBAL_TEAM_ID {
        board.properties.insert(
            "trackingTemplateId".to_owned(),
            Value::String(md5_hex(&board.title)),
        );
    }
    let exists = find_board(scope, &board.id).await?.is_some();

    let now = now_millis();
    user_id.clone_into(&mut board.modified_by);
    board.update_at = now;

    if exists {
        // Preserved quirk: an update does not touch the stored team, creator and creation
        // time, yet `board` keeps whatever the caller put there, and that is what the
        // history row below records (`server/services/store/sqlstore/board.go:339-399`).
        let mut sql = scope.sql(&format!("UPDATE {} SET ", scope.table("boards")));
        sql.push_bind("modified_by = ", &board.modified_by)
            .push_bind(", type = ", board.board_type.as_str())
            .push_bind(", channel_id = ", &board.channel_id)
            .push_bind(", minimum_role = ", board.minimum_role.as_str())
            .push_bind(", title = ", &board.title)
            .push_bind(", description = ", &board.description)
            .push_bind(", icon = ", &board.icon)
            .push_bind(", show_description = ", board.show_description)
            .push_bind(", is_template = ", board.is_template)
            .push_bind(", template_version = ", board.template_version)
            .push_bind(", properties = ", Param::jsonb(&board.properties)?)
            .push_bind(
                ", card_properties = ",
                Param::jsonb(&board.card_properties)?,
            )
            .push_bind(", update_at = ", board.update_at)
            .push_bind(", delete_at = ", board.delete_at)
            .push_bind(" WHERE id = ", &board.id);
        scope.execute(sql).await?;
    } else {
        user_id.clone_into(&mut board.created_by);
        board.create_at = now;
        insert_board_row(scope, BoardTable::Boards, board).await?;
    }
    insert_board_row(scope, BoardTable::History, board).await
}

/// The lowercase hexadecimal MD5 digest of `text`.
fn md5_hex(text: &str) -> String {
    let digest = Md5::digest(text.as_bytes());
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        // Writing to a `String` cannot fail.
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// One of the two tables that hold board rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BoardTable {
    /// The current state of every board.
    Boards,
    /// Every recorded state of every board.
    History,
}

/// Inserts `board` as it is into one of the board tables.
async fn insert_board_row(
    scope: &mut Scope<'_>,
    table: BoardTable,
    board: &Board,
) -> Result<(), StoreError> {
    let (name, stamp) = match table {
        BoardTable::Boards => ("boards", None),
        BoardTable::History => ("boards_history", Some(scope.history_stamp())),
    };
    let mut sql = scope.sql(&format!(
        "INSERT INTO {} ({BOARD_COLUMNS}{}) VALUES (",
        scope.table(name),
        if stamp.is_some() { ", insert_at" } else { "" }
    ));
    sql.bind(&board.id)
        .push_bind(", ", &board.team_id)
        .push_bind(", ", &board.channel_id)
        .push_bind(", ", &board.created_by)
        .push_bind(", ", &board.modified_by)
        .push_bind(", ", board.board_type.as_str())
        .push_bind(", ", board.minimum_role.as_str())
        .push_bind(", ", &board.title)
        .push_bind(", ", &board.description)
        .push_bind(", ", &board.icon)
        .push_bind(", ", board.show_description)
        .push_bind(", ", board.is_template)
        .push_bind(", ", board.template_version)
        .push_bind(", ", Param::jsonb(&board.properties)?)
        .push_bind(", ", Param::jsonb(&board.card_properties)?)
        .push_bind(", ", board.create_at)
        .push_bind(", ", board.update_at)
        .push_bind(", ", board.delete_at);
    if let Some(stamp) = stamp {
        sql.push(", ").bind_history_stamp(stamp);
    }
    sql.push(")");
    scope.execute(sql).await?;
    Ok(())
}

pub(crate) async fn insert_board_with_admin(
    scope: &mut Scope<'_>,
    board: &mut Board,
    user_id: &str,
) -> Result<BoardMember, StoreError> {
    insert_board(scope, board, user_id).await?;
    // Preserved quirk: the administrator is the board's creator as `board` names it. For
    // a new board that is `user_id`; for an update it is whatever the caller put there
    // (`server/services/store/sqlstore/board.go:487-492`).
    let member = BoardMember {
        board_id: board.id.clone(),
        user_id: board.created_by.clone(),
        scheme_admin: true,
        scheme_editor: true,
        ..BoardMember::default()
    };
    save_member(scope, &member).await?;
    Ok(member)
}

pub(crate) async fn patch_board(
    scope: &mut Scope<'_>,
    board_id: &str,
    patch: &BoardPatch,
    user_id: &str,
) -> Result<Board, StoreError> {
    // Before the board is looked at: applying the patch would drop a definition without
    // an identifier silently.
    patch.validate().map_err(BoardError::from)?;
    let mut board = get_board(scope, board_id).await?;
    patch.apply(&mut board);
    insert_board(scope, &mut board, user_id).await?;
    Ok(board)
}

pub(crate) async fn delete_board(
    scope: &mut Scope<'_>,
    board_id: &str,
    user_id: &str,
) -> Result<(), StoreError> {
    let mut board = get_board(scope, board_id).await?;

    let now = now_millis();
    user_id.clone_into(&mut board.modified_by);
    board.update_at = now;
    board.delete_at = now;
    insert_board_row(scope, BoardTable::History, &board).await?;

    let mut sql = scope.sql(&format!(
        "DELETE FROM {} WHERE id = ",
        scope.table("boards")
    ));
    sql.bind(board_id)
        .push_bind(" AND COALESCE(team_id, '0') = ", &board.team_id);
    scope.execute(sql).await?;

    blocks::delete_block_children(scope, board_id, None, user_id).await
}

pub(crate) async fn undelete_board(
    scope: &mut Scope<'_>,
    board_id: &str,
    modified_by: &str,
) -> Result<(), StoreError> {
    let newest = get_board_history(scope, board_id, &HistoryQuery::NEWEST).await?;
    let Some(mut board) = newest.into_iter().next() else {
        tracing::warn!(board_id, "undelete_board: board not found");
        return Ok(());
    };
    if board.delete_at == 0 {
        tracing::warn!(board_id, "undelete_board: board not deleted");
        return Ok(());
    }

    // Preserved quirk: the board comes back without its channel
    // (`server/services/store/sqlstore/board.go:838-842`).
    board.channel_id.clear();
    modified_by.clone_into(&mut board.modified_by);
    board.update_at = now_millis();
    board.delete_at = 0;
    insert_board_row(scope, BoardTable::History, &board).await?;
    insert_board_row(scope, BoardTable::Boards, &board).await?;

    blocks::undelete_block_children(scope, &board.id, None, modified_by).await
}

pub(crate) async fn get_board_count(scope: &mut Scope<'_>) -> Result<i64, StoreError> {
    let mut sql = scope.sql(&format!(
        "SELECT COUNT(*) FROM {} WHERE delete_at = ",
        scope.table("boards")
    ));
    sql.bind(0_i64).push_bind(" AND is_template = ", false);
    match scope.fetch_optional(sql).await? {
        Some(row) => row.int(0),
        None => Ok(0),
    }
}

pub(crate) async fn get_board_history(
    scope: &mut Scope<'_>,
    board_id: &str,
    query: &HistoryQuery,
) -> Result<Vec<Board>, StoreError> {
    let mut sql = select_boards(scope, "boards_history");
    sql.push_bind(" WHERE id = ", board_id);
    if let Some(before) = query.before_update_at {
        sql.push_bind(" AND update_at < ", before);
    }
    if let Some(after) = query.after_update_at {
        sql.push_bind(" AND update_at > ", after);
    }
    sql.push(match query.order {
        HistoryOrder::Ascending => " ORDER BY insert_at, update_at",
        HistoryOrder::Descending => " ORDER BY insert_at DESC, update_at DESC",
    });
    if let Some(limit) = query.limit {
        sql.push_bind(" LIMIT ", i64::from(limit));
    }
    fetch_boards(scope, sql).await
}

/// Starts the query for memberships, each with the minimum role of its board.
fn select_members(scope: &Scope<'_>) -> Sql {
    scope.sql(&format!(
        "SELECT b.minimum_role, bm.board_id, bm.user_id, bm.roles, bm.scheme_admin, \
         bm.scheme_editor, bm.scheme_commenter, bm.scheme_viewer \
         FROM {} AS bm LEFT JOIN {} AS b ON b.id = bm.board_id",
        scope.table("board_members"),
        scope.table("boards")
    ))
}

async fn fetch_members(scope: &mut Scope<'_>, sql: Sql) -> Result<Vec<BoardMember>, StoreError> {
    scope
        .fetch_all(sql)
        .await?
        .iter()
        .map(|row| {
            Ok(BoardMember {
                // Empty when the board does not exist, or no longer exists.
                minimum_role: row.text(0)?,
                board_id: row.text(1)?,
                user_id: row.text(2)?,
                roles: row.text(3)?,
                scheme_admin: row.bool(4)?,
                scheme_editor: row.bool(5)?,
                scheme_commenter: row.bool(6)?,
                scheme_viewer: row.bool(7)?,
                synthetic: false,
            })
        })
        .collect()
}

async fn find_member(
    scope: &mut Scope<'_>,
    board_id: &str,
    user_id: &str,
) -> Result<Option<BoardMember>, StoreError> {
    let mut sql = select_members(scope);
    sql.push_bind(" WHERE bm.board_id = ", board_id)
        .push_bind(" AND bm.user_id = ", user_id);
    Ok(fetch_members(scope, sql).await?.into_iter().next())
}

pub(crate) async fn get_member_for_board(
    scope: &mut Scope<'_>,
    board_id: &str,
    user_id: &str,
) -> Result<BoardMember, StoreError> {
    find_member(scope, board_id, user_id).await?.ok_or_else(|| {
        StoreError::not_found(format_args!(
            "board member BoardID={board_id} UserID={user_id}"
        ))
    })
}

pub(crate) async fn get_members_for_user(
    scope: &mut Scope<'_>,
    user_id: &str,
) -> Result<Vec<BoardMember>, StoreError> {
    let mut sql = select_members(scope);
    sql.push_bind(" WHERE bm.user_id = ", user_id);
    fetch_members(scope, sql).await
}

pub(crate) async fn get_members_for_board(
    scope: &mut Scope<'_>,
    board_id: &str,
) -> Result<Vec<BoardMember>, StoreError> {
    let mut sql = select_members(scope);
    sql.push_bind(" WHERE bm.board_id = ", board_id);
    fetch_members(scope, sql).await
}

pub(crate) async fn save_member(
    scope: &mut Scope<'_>,
    member: &BoardMember,
) -> Result<(), StoreError> {
    let is_new = find_member(scope, &member.board_id, &member.user_id)
        .await?
        .is_none();

    // Preserved quirk: the member's `roles` are never stored. A new membership gets
    // empty roles and an existing one keeps what it has
    // (`server/services/store/sqlstore/board.go:503-532`).
    let mut sql = scope.sql(&format!(
        "INSERT INTO {} (board_id, user_id, roles, scheme_admin, scheme_editor, \
         scheme_commenter, scheme_viewer) VALUES (",
        scope.table("board_members")
    ));
    sql.bind(&member.board_id)
        .push_bind(", ", &member.user_id)
        .push_bind(", ", "")
        .push_bind(", ", member.scheme_admin)
        .push_bind(", ", member.scheme_editor)
        .push_bind(", ", member.scheme_commenter)
        .push_bind(", ", member.scheme_viewer)
        .push(
            ") ON CONFLICT (board_id, user_id) DO UPDATE SET \
             scheme_admin = EXCLUDED.scheme_admin, scheme_editor = EXCLUDED.scheme_editor, \
             scheme_commenter = EXCLUDED.scheme_commenter, \
             scheme_viewer = EXCLUDED.scheme_viewer",
        );
    scope.execute(sql).await?;

    if is_new {
        record_membership(scope, &member.board_id, &member.user_id, "created").await?;
    }
    Ok(())
}

pub(crate) async fn delete_member(
    scope: &mut Scope<'_>,
    board_id: &str,
    user_id: &str,
) -> Result<(), StoreError> {
    let mut sql = scope.sql(&format!(
        "DELETE FROM {} WHERE board_id = ",
        scope.table("board_members")
    ));
    sql.bind(board_id).push_bind(" AND user_id = ", user_id);
    if scope.execute(sql).await? > 0 {
        record_membership(scope, board_id, user_id, "deleted").await?;
        // Whoever may no longer see the board is no longer told what changes on it.
        subscriptions::end_subscriptions_to_board(scope, board_id, user_id).await?;
    }
    Ok(())
}

/// Appends an entry to the membership history.
async fn record_membership(
    scope: &mut Scope<'_>,
    board_id: &str,
    user_id: &str,
    action: &str,
) -> Result<(), StoreError> {
    let stamp = scope.history_stamp();
    let mut sql = scope.sql(&format!(
        "INSERT INTO {} (board_id, user_id, action, insert_at) VALUES (",
        scope.table("board_members_history")
    ));
    sql.bind(board_id)
        .push_bind(", ", user_id)
        .push_bind(", ", action)
        .push(", ")
        .bind_history_stamp(stamp)
        .push(")");
    scope.execute(sql).await?;
    Ok(())
}

pub(crate) async fn get_board_member_history(
    scope: &mut Scope<'_>,
    board_id: &str,
    user_id: &str,
    limit: Option<u32>,
) -> Result<Vec<BoardMemberHistoryEntry>, StoreError> {
    // `insert_at` is text on SQLite and a timestamp on PostgreSQL; both are read as
    // milliseconds since the epoch.
    let insert_at_millis = match scope.dialect() {
        Dialect::Sqlite => {
            "CAST(STRFTIME('%s', insert_at) AS INTEGER) * 1000 \
             + CAST(SUBSTR(STRFTIME('%f', insert_at), 4) AS INTEGER)"
        }
        Dialect::Postgres => "CAST(FLOOR(EXTRACT(EPOCH FROM insert_at) * 1000) AS BIGINT)",
    };
    let mut sql = scope.sql(&format!(
        "SELECT board_id, user_id, action, {insert_at_millis} FROM {} WHERE board_id = ",
        scope.table("board_members_history")
    ));
    sql.bind(board_id)
        .push_bind(" AND user_id = ", user_id)
        .push(" ORDER BY insert_at DESC");
    if let Some(limit) = limit {
        sql.push_bind(" LIMIT ", i64::from(limit));
    }
    scope
        .fetch_all(sql)
        .await?
        .iter()
        .map(|row| {
            Ok(BoardMemberHistoryEntry {
                board_id: row.text(0)?,
                user_id: row.text(1)?,
                action: row.text(2)?,
                insert_at: row.int(3)?,
            })
        })
        .collect()
}

/// Appends one condition per space-separated word of `term`: the board's title must
/// contain the word, ignoring case.
fn push_title_words(sql: &mut Sql, term: &str) {
    // Preserved quirk: the words are split at single spaces, so two spaces in a row
    // yield an empty word that matches every title, and `%` and `_` in a word are `LIKE`
    // wildcards. On SQLite `lower()` only folds ASCII letters, so a title with other
    // capital letters is not found (`server/services/store/sqlstore/board.go:691-695`).
    for word in term.trim().split(' ') {
        sql.push_bind(
            " AND lower(b.title) LIKE ",
            format!("%{}%", word.to_lowercase()),
        );
    }
}

pub(crate) async fn search_boards_for_user(
    scope: &mut Scope<'_>,
    term: &str,
    search_field: BoardSearchField,
    user_id: &str,
    public_boards: PublicBoards,
) -> Result<Vec<Board>, StoreError> {
    let mut sql = select_boards_of_user(scope, user_id, public_boards);
    if !term.is_empty() {
        match search_field {
            BoardSearchField::Title => push_title_words(&mut sql, term),
            BoardSearchField::PropertyName => match scope.dialect() {
                Dialect::Postgres => {
                    sql.push_bind(" AND b.properties -> ", term)
                        .push(" IS NOT NULL");
                }
                Dialect::Sqlite => {
                    // Preserved quirk: the term becomes part of a JSON path, so on SQLite
                    // `a.b` finds a nested property, a property set to `null` is not
                    // found, and a term that is not a valid path is a database error
                    // (`server/services/store/sqlstore/board.go:678-680`).
                    sql.push_bind(" AND JSON_EXTRACT(b.properties, ", format!("$.{term}"))
                        .push(") IS NOT NULL");
                }
            },
        }
    }
    fetch_boards(scope, sql).await
}

pub(crate) async fn search_boards_for_user_in_team(
    scope: &mut Scope<'_>,
    team_id: &str,
    term: &str,
    user_id: &str,
) -> Result<Vec<Board>, StoreError> {
    let mut sql = scope.sql(&format!(
        "SELECT DISTINCT {} FROM {} AS b LEFT JOIN {} AS bm ON b.id = bm.board_id \
         WHERE b.is_template = ",
        board_select_list("b."),
        scope.table("boards"),
        scope.table("board_members")
    ));
    sql.bind(false)
        .push_bind(" AND b.team_id = ", team_id)
        .push_bind(" AND (b.type = ", BoardType::Open.as_str())
        .push_bind(" OR (b.type = ", BoardType::Private.as_str())
        .push_bind(" AND bm.user_id = ", user_id)
        .push("))");
    if !term.is_empty() {
        push_title_words(&mut sql, term);
    }
    fetch_boards(scope, sql).await
}

pub(crate) async fn get_template_boards(
    scope: &mut Scope<'_>,
    team_id: &str,
    user_id: &str,
) -> Result<Vec<Board>, StoreError> {
    let mut sql = scope.sql(&format!(
        "SELECT {} FROM {} AS b LEFT JOIN {} AS bm ON b.id = bm.board_id AND bm.user_id = ",
        board_select_list("b."),
        scope.table("boards"),
        scope.table("board_members")
    ));
    // Open templates are included even without a membership.
    sql.bind(user_id)
        .push_bind(" WHERE b.is_template = ", true)
        .push_bind(" AND b.team_id = ", team_id)
        .push_bind(
            " AND ((bm.board_id IS NULL AND b.type = ",
            BoardType::Open.as_str(),
        )
        .push(") OR bm.board_id IS NOT NULL)");
    fetch_boards(scope, sql).await
}

pub(crate) async fn remove_default_templates(
    scope: &mut Scope<'_>,
    boards: &[Board],
) -> Result<(), StoreError> {
    let mut count = 0_u32;
    for board in boards {
        if board.created_by != SYSTEM_USER_ID {
            continue;
        }
        // Default templates are removed without a trace in the history tables.
        let mut sql = scope.sql(&format!(
            "DELETE FROM {} WHERE id = ",
            scope.table("boards")
        ));
        sql.bind(&board.id).push_bind(" AND is_template = ", true);
        scope.execute(sql).await?;

        // Preserved quirk: the blocks go even when the board stayed because it is not a
        // template (`server/services/store/sqlstore/templates.go:24-41`).
        let mut sql = scope.sql(&format!(
            "DELETE FROM {} WHERE parent_id = ",
            scope.table("blocks")
        ));
        sql.bind(&board.id)
            .push_bind(" OR root_id = ", &board.id)
            .push_bind(" OR board_id = ", &board.id);
        scope.execute(sql).await?;

        tracing::trace!(board_id = %board.id, "removed default template");
        count += 1;
    }
    tracing::debug!(count, "removed default templates");
    Ok(())
}
