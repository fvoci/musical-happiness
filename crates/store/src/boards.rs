//! The boards that exist, board membership and board templates.
//!
//! Deleting a board moves it to the trash and restoring brings it back; both are in
//! [`crate::trash`]. Memberships are kept in a table of their own and go with their
//! board.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use fvoci_model::{
    Board, BoardError, BoardKind, BoardKinds, BoardMember, BoardPatch, BoardRole, BoardType,
    JsonMap, MinimumRole,
};
use md5::{Digest as _, Md5};
use serde_json::Value;

use crate::Store;
use crate::db::{Dialect, Id, OptId, Param, Row, Scope, Sql, now_millis};
use crate::error::StoreError;
use crate::{board_kinds, members, subscriptions};

/// The order of a list of boards read through the alias `b`: by when they were made,
/// and by identifier where that is the same.
pub(crate) const OLDEST_BOARD_FIRST: &str = " ORDER BY b.create_at, b.id";

/// The name of every board column, in the order rows are written.
const BOARD_COLUMNS: &str = "id, workspace_id, created_by, modified_by, type, minimum_role, title, \
                             description, icon, show_description, is_template, is_builtin, \
                             template_version, properties, card_properties, create_at, \
                             update_at, kind";

/// Whether a board query also returns the open boards the user is not a member of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicBoards {
    /// Open boards are returned whether or not the user is a member.
    Include,
    /// Only boards the user is a member of are returned.
    Exclude,
}

/// What joining a board did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Joined {
    /// The user was no member and is one now, with the board's minimum role.
    New(BoardMember),
    /// The user was a member below the board's minimum role and has that role now.
    Raised(BoardMember),
    /// The user was a member with the minimum role or a higher one; nothing changed.
    Already(BoardMember),
}

impl Joined {
    /// The membership as it is after joining.
    pub fn into_member(self) -> BoardMember {
        match self {
            Self::New(member) | Self::Raised(member) | Self::Already(member) => member,
        }
    }
}

/// The highest role the scheme flags of a membership give; none if no flag is set.
fn highest_role(member: &BoardMember) -> Option<BoardRole> {
    if member.scheme_admin {
        Some(BoardRole::Admin)
    } else if member.scheme_editor {
        Some(BoardRole::Editor)
    } else if member.scheme_commenter {
        Some(BoardRole::Commenter)
    } else if member.scheme_viewer {
        Some(BoardRole::Viewer)
    } else {
        None
    }
}

/// What became of a change to a membership that must leave its board an administrator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemberChange {
    /// The change was made.
    Done,
    /// The user is no member of the board; nothing changed.
    NotAMember,
    /// The user is the board's only administrator and the change would have ended
    /// that; nothing changed.
    LastAdmin,
}

impl Store {
    /// Inserts `board`, or updates the board that already has its identifier.
    ///
    /// `board` is updated to what was stored: `user_id` as modifier and the current time
    /// as modification time and, for a new board, `user_id` as creator and the current
    /// time as creation time. An update leaves the stored workspace, creator and creation time
    /// alone, and `board` comes back with those.
    ///
    /// A template stored on behalf of no user (the empty `user_id`) is a built-in
    /// template: `is_builtin` is set, whatever `board` says, and it gets a
    /// `trackingTemplateId` property derived from its title. No other board is built-in,
    /// and an update never changes whether a board is.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::InvalidBoard`], and nothing written, for a board that may
    /// not be stored ([`Board::check_storable`]) and for one that exists as of another
    /// kind; [`StoreError::Database`] if a statement fails.
    pub async fn insert_board(&self, board: &mut Board, user_id: &str) -> Result<(), StoreError> {
        self.transaction(async |scope| insert_board(scope, board, user_id).await)
            .await
    }

    /// Inserts or updates `board` as [`Store::insert_board`] does and makes its creator
    /// an administrator and editor of it. Returns that membership.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if its creator does not work there, or a database error.
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

    /// Puts the card properties of a board into the order of their identifiers in
    /// `order`, which names each of them once, and returns the board, marked as modified
    /// by `user_id` now. The board is held from before it is read.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if there is no such board, and
    /// [`StoreError::InvalidBoard`], and nothing written, if `order` does not name each
    /// property once ([`Board::reorder_card_properties`]).
    pub async fn reorder_card_properties(
        &self,
        board_id: &str,
        order: &[String],
        user_id: &str,
    ) -> Result<Board, StoreError> {
        self.transaction(async |scope| {
            hold_board(scope, board_id).await?;
            let mut board = get_board(scope, board_id).await?;
            board.reorder_card_properties(order)?;
            insert_board(scope, &mut board, user_id).await?;
            Ok(board)
        })
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

    /// Returns the boards of `kinds` of a workspace that a user is a member of and, with
    /// [`PublicBoards::Include`], the open ones of the workspace, oldest first. Templates
    /// are left out.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn get_boards_for_user_and_workspace(
        &self,
        user_id: &str,
        workspace_id: &str,
        public_boards: PublicBoards,
        kinds: BoardKinds,
    ) -> Result<Vec<Board>, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = select_boards_of_user(scope, user_id, public_boards, kinds);
            sql.push_bind(" AND b.workspace_id = ", Id(workspace_id))
                .push(OLDEST_BOARD_FIRST);
            fetch_boards(scope, sql).await
        })
        .await
    }

    /// Returns the boards of a workspace with the given identifiers, oldest first.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotAllFound`] unless exactly one board is found per identifier.
    pub async fn get_boards_in_workspace_by_ids(
        &self,
        board_ids: &[String],
        workspace_id: &str,
    ) -> Result<Vec<Board>, StoreError> {
        let ids = board_ids;
        self.with_conn(async |scope| get_boards_in_workspace_by_ids(scope, ids, workspace_id).await)
            .await
    }

    /// Makes a user a member of a board with the scheme flags of `member`, or replaces
    /// the scheme flags of an existing membership, whatever that does to the board: for
    /// tests that need a membership the checked ways would not make. The server uses
    /// [`Store::add_member`], [`Store::join_board`] and [`Store::change_member`].
    ///
    /// The `synthetic` member of `member` is not stored; `roles` has no column and reads
    /// as empty.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if a statement fails.
    #[cfg(feature = "test-util")]
    pub async fn save_member(&self, member: &BoardMember) -> Result<(), StoreError> {
        self.transaction(async |scope| save_member(scope, member).await)
            .await
    }

    /// Removes a user from a board and ends the user's subscriptions to the blocks of
    /// the board, also if that leaves the board without an administrator: for tests.
    /// The server uses [`Store::remove_member`]. Removing a user who is not a member
    /// succeeds and changes nothing.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if a statement fails.
    #[cfg(feature = "test-util")]
    pub async fn delete_member(&self, board_id: &str, user_id: &str) -> Result<(), StoreError> {
        self.transaction(async |scope| delete_member(scope, board_id, user_id).await)
            .await
    }

    /// Makes a user a member of a board with the board's minimum role, as joining does.
    ///
    /// A user who is no member becomes one with that role. A member whose role is below
    /// it is raised to it. A member with that role or a higher one stays as they are:
    /// joining never lowers anybody.
    ///
    /// Reading the board and the membership and writing the membership are one step.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if there is no such board or no such user.
    pub async fn join_board(&self, board_id: &str, user_id: &str) -> Result<Joined, StoreError> {
        self.transaction(async |scope| {
            // Nobody joins a board the handle reads and may not change (a built-in template).
            hold_board(scope, board_id).await?;
            let board = get_board(scope, board_id).await?;
            let minimum = BoardRole::from(board.minimum_role);
            let with_minimum = BoardMember {
                board_id: board_id.to_owned(),
                user_id: user_id.to_owned(),
                scheme_editor: minimum == BoardRole::Editor,
                scheme_commenter: minimum == BoardRole::Commenter,
                scheme_viewer: minimum == BoardRole::Viewer,
                ..BoardMember::default()
            };
            match find_member(scope, board_id, user_id).await? {
                None => {
                    save_member(scope, &with_minimum)
                        .await
                        .map_err(|error| error.missing_reference("user"))?;
                    Ok(Joined::New(with_minimum))
                }
                Some(member) if highest_role(&member) < Some(minimum) => {
                    save_member(scope, &with_minimum).await?;
                    Ok(Joined::Raised(with_minimum))
                }
                Some(member) => Ok(Joined::Already(member)),
            }
        })
        .await
    }

    /// Removes a user from a board and ends the user's subscriptions to the blocks of
    /// the board, unless that would leave the board without an administrator.
    ///
    /// The check and the removal are one step: of two administrators who leave at once,
    /// one is refused.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if a statement fails.
    pub async fn remove_member(
        &self,
        board_id: &str,
        user_id: &str,
    ) -> Result<MemberChange, StoreError> {
        self.transaction(async |scope| {
            if !lock_board(scope, board_id).await? {
                return Ok(MemberChange::NotAMember);
            }
            let Some(current) = find_member(scope, board_id, user_id).await? else {
                return Ok(MemberChange::NotAMember);
            };
            if current.scheme_admin && members::is_only_admin(scope, board_id, user_id).await? {
                return Ok(MemberChange::LastAdmin);
            }
            delete_member(scope, board_id, user_id).await?;
            Ok(MemberChange::Done)
        })
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

    /// Returns the members of a board, by the identifier of the user: the users in the
    /// order their accounts were made in.
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

    /// Returns a user's memberships, by the identifier of the board: the boards in the
    /// order they were made in. A membership of a board of a workspace the user is no
    /// member of any more is left out.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn get_members_for_user(
        &self,
        user_id: &str,
    ) -> Result<Vec<BoardMember>, StoreError> {
        self.with_conn(async |scope| {
            crate::board_team_access::get_members_for_user(scope, user_id).await
        })
        .await
    }

    /// Returns the templates of a workspace that are open or that the user is a member of,
    /// oldest first. The built-in templates are not among them.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn get_template_boards(
        &self,
        workspace_id: &str,
        user_id: &str,
    ) -> Result<Vec<Board>, StoreError> {
        self.with_conn(async |scope| {
            crate::workspaces::get_template_boards(scope, workspace_id, user_id).await
        })
        .await
    }

    /// Returns the templates the server ships with, oldest first. They are the system
    /// organization's, in its workspace: every organization is offered them.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn get_builtin_templates(&self) -> Result<Vec<Board>, StoreError> {
        self.with_conn(get_builtin_templates).await
    }

    /// Removes the built-in templates together with their blocks, for good: they do not
    /// go to the trash. For tests: it neither takes the lock of
    /// [`Store::replace_builtin_templates`] nor hands back the templates' files, so the
    /// server replaces its templates with that.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if a statement fails; nothing is removed then.
    pub async fn remove_builtin_templates(&self) -> Result<(), StoreError> {
        self.transaction(remove_builtin_templates).await
    }
}

/// The select list of a board, in the order [`board_from_row`] reads it. `alias` is
/// empty or a table alias followed by a dot.
pub(crate) fn board_select_list(alias: &str) -> String {
    format!(
        "{alias}id, {alias}workspace_id, {alias}created_by, {alias}modified_by, {alias}type, \
         {alias}minimum_role, {alias}title, {alias}description, {alias}icon, \
         {alias}show_description, {alias}is_template, {alias}is_builtin, \
         {alias}template_version, {alias}properties, {alias}card_properties, \
         {alias}create_at, {alias}update_at, {alias}kind"
    )
}

/// Starts `SELECT <board columns> FROM boards`.
fn select_boards(scope: &Scope<'_>) -> Sql {
    scope.sql(&format!("SELECT {} FROM boards", board_select_list("")))
}

pub(crate) async fn fetch_boards(
    scope: &mut Scope<'_>,
    sql: Sql,
) -> Result<Vec<Board>, StoreError> {
    scope
        .fetch_all(sql)
        .await?
        .iter()
        .map(board_from_row)
        .collect()
}

fn board_from_row(row: &Row) -> Result<Board, StoreError> {
    Ok(Board {
        id: row.id(0)?,
        workspace_id: row.id(1)?,
        created_by: row.id(2)?,
        modified_by: row.id(3)?,
        board_type: board_type_from_text(&row.text(4)?)?,
        kind: board_kinds::kind_from_text(&row.text(17)?)?,
        minimum_role: minimum_role_from_text(&row.text(5)?)?,
        title: row.text(6)?,
        description: row.text(7)?,
        icon: row.text(8)?,
        show_description: row.bool(9)?,
        is_template: row.bool(10)?,
        is_builtin: row.bool(11)?,
        template_version: row.int(12)?,
        properties: row.json_map(13, "properties")?,
        card_properties: card_properties_from_json(row.json(14, "card_properties")?)?,
        create_at: row.int(15)?,
        update_at: row.int(16)?,
        // A board that exists is not deleted.
        delete_at: 0,
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

fn minimum_role_from_text(text: &str) -> Result<MinimumRole, StoreError> {
    MinimumRole::ALL
        .into_iter()
        .find(|role| role.as_str() == text)
        .ok_or_else(|| StoreError::Corrupt {
            column: "minimum_role",
            reason: format!("unknown minimum role {text:?}"),
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

pub(crate) async fn find_board(
    scope: &mut Scope<'_>,
    board_id: &str,
) -> Result<Option<Board>, StoreError> {
    let mut sql = select_boards(scope);
    sql.push_bind(" WHERE id = ", Id(board_id));
    Ok(fetch_boards(scope, sql).await?.into_iter().next())
}

pub(crate) async fn get_board(scope: &mut Scope<'_>, board_id: &str) -> Result<Board, StoreError> {
    // Preserved quirk: the error names the table rather than the board that is missing
    // (`server/services/store/sqlstore/board.go:227`).
    find_board(scope, board_id)
        .await?
        .ok_or_else(|| StoreError::not_found("boards"))
}

/// Starts the query for the boards of `kinds` a user can reach: `SELECT DISTINCT` over
/// boards joined with their memberships, without templates, narrowed to the user's boards
/// and, if asked for, the open ones where the user works. A membership counts while the
/// user is a member of the board's workspace, and if it has a role. More `AND` conditions
/// on `b` may follow.
pub(crate) fn select_boards_of_user(
    scope: &Scope<'_>,
    user_id: &str,
    public_boards: PublicBoards,
    kinds: BoardKinds,
) -> Sql {
    let mut sql = scope.sql(&format!(
        "SELECT DISTINCT {} FROM boards AS b LEFT JOIN board_members AS bm ON b.id = bm.board_id \
         WHERE b.is_template = ",
        board_select_list("b.")
    ));
    sql.bind(false);
    board_kinds::push_kinds(&mut sql, "b.kind", kinds);
    match public_boards {
        PublicBoards::Include => members::push_open_boards_at_work(&mut sql, user_id),
        PublicBoards::Exclude => sql.push(" AND ("),
    };
    sql.push("(");
    members::push_own_membership_in_workspace(&mut sql, user_id)
        .push(members::BM_HAS_A_ROLE)
        .push(") OR ");
    crate::board_team_access::push_has_team_role(&mut sql, scope, user_id);
    sql.push(")");
    sql
}

pub(crate) async fn get_boards_in_workspace_by_ids(
    scope: &mut Scope<'_>,
    board_ids: &[String],
    workspace_id: &str,
) -> Result<Vec<Board>, StoreError> {
    let mut sql = select_boards(scope);
    sql.push_bind(" WHERE workspace_id = ", Id(workspace_id))
        .push(" AND id IN (")
        .bind_list(board_ids.iter().map(|id| Id(id)))
        .push(") ORDER BY create_at, id");
    let boards = fetch_boards(scope, sql).await?;
    if boards.len() == board_ids.len() {
        Ok(boards)
    } else {
        tracing::warn!(
            found = boards.len(),
            asked = board_ids.len(),
            "get_boards_in_workspace_by_ids: mismatched number of boards found"
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
    board.check_storable()?;
    // A board that exists is held while it is read and written.
    lock_board(scope, &board.id).await?;
    let existing = find_board(scope, &board.id).await?;
    // A template stored on behalf of no user is the system's own: a built-in template.
    // Nothing a caller puts into `board` changes that, and it never changes afterwards.
    board.is_builtin = match &existing {
        Some(existing) => existing.is_builtin,
        None => user_id.is_empty() && board.is_template,
    };
    // The built-in templates are told apart by a hash of their title.
    if board.is_template && board.is_builtin {
        board.properties.insert(
            "trackingTemplateId".to_owned(),
            Value::String(md5_hex(&board.title)),
        );
    }
    let now = now_millis();
    user_id.clone_into(&mut board.modified_by);
    board.update_at = now;
    board.delete_at = 0;

    if let Some(existing) = existing {
        // A board is of the kind it was made as: nothing stores it as of another.
        if existing.kind != board.kind {
            return Err(BoardError::KindChanges {
                stored: existing.kind,
                asked: board.kind,
            }
            .into());
        }
        // A board stays in its workspace, and who made it and when does not change: `board`
        // comes back with what is stored, whatever the caller put there.
        board.workspace_id = existing.workspace_id;
        board.created_by = existing.created_by;
        board.create_at = existing.create_at;
        let mut sql = scope.sql("UPDATE boards SET ");
        sql.push_bind("modified_by = ", OptId(&board.modified_by))
            .push_bind(", type = ", board.board_type.as_str())
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
            .push_bind(" WHERE id = ", Id(&board.id));
        if scope.execute(sql).await? == 0 {
            return Err(StoreError::not_found(format_args!("board ID={}", board.id)));
        }
        Ok(())
    } else {
        user_id.clone_into(&mut board.created_by);
        board.create_at = now;
        insert_board_row(scope, board).await
    }
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

/// Inserts a board row with every column as `board` has it. For a new board, and for
/// one that comes back from the trash.
pub(crate) async fn insert_board_row(
    scope: &mut Scope<'_>,
    board: &Board,
) -> Result<(), StoreError> {
    let mut sql = scope.sql(&format!("INSERT INTO boards ({BOARD_COLUMNS}) VALUES ("));
    sql.bind(Id(&board.id))
        .push_bind(", ", Id(&board.workspace_id))
        .push_bind(", ", OptId(&board.created_by))
        .push_bind(", ", OptId(&board.modified_by))
        .push_bind(", ", board.board_type.as_str())
        .push_bind(", ", board.minimum_role.as_str())
        .push_bind(", ", &board.title)
        .push_bind(", ", &board.description)
        .push_bind(", ", &board.icon)
        .push_bind(", ", board.show_description)
        .push_bind(", ", board.is_template)
        .push_bind(", ", board.is_builtin)
        .push_bind(", ", board.template_version)
        .push_bind(", ", Param::jsonb(&board.properties)?)
        .push_bind(", ", Param::jsonb(&board.card_properties)?)
        .push_bind(", ", board.create_at)
        .push_bind(", ", board.update_at)
        .push_bind(", ", board.kind.as_str())
        .push(")");
    scope.execute(sql).await?;
    Ok(())
}

pub(crate) async fn insert_board_with_admin(
    scope: &mut Scope<'_>,
    board: &mut Board,
    user_id: &str,
) -> Result<BoardMember, StoreError> {
    insert_board(scope, board, user_id).await?;
    // Its admin is its creator (`user_id`, or the stored one of a board that existed).
    members::hold_creator(scope, &board.workspace_id, &board.created_by).await?;
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
    // Held before the board is read: of two patches at once, the second is applied to
    // what the first left.
    hold_board(scope, board_id).await?;
    let mut board = get_board(scope, board_id).await?;
    patch.apply(&mut board);
    insert_board(scope, &mut board, user_id).await?;
    Ok(board)
}

/// Starts the query for memberships.
pub(crate) fn select_members(scope: &Scope<'_>) -> Sql {
    scope.sql(
        "SELECT bm.board_id, bm.user_id, bm.scheme_admin, bm.scheme_editor, \
         bm.scheme_commenter, bm.scheme_viewer FROM board_members AS bm",
    )
}

pub(crate) async fn fetch_members(
    scope: &mut Scope<'_>,
    sql: Sql,
) -> Result<Vec<BoardMember>, StoreError> {
    scope
        .fetch_all(sql)
        .await?
        .iter()
        .map(|row| {
            Ok(BoardMember {
                board_id: row.id(0)?,
                user_id: row.id(1)?,
                roles: String::new(),
                scheme_admin: row.bool(2)?,
                scheme_editor: row.bool(3)?,
                scheme_commenter: row.bool(4)?,
                scheme_viewer: row.bool(5)?,
                synthetic: false,
                inherited_role: None,
            })
        })
        .collect()
}

pub(crate) async fn find_member(
    scope: &mut Scope<'_>,
    board_id: &str,
    user_id: &str,
) -> Result<Option<BoardMember>, StoreError> {
    let mut sql = select_members(scope);
    sql.push_bind(" WHERE bm.board_id = ", Id(board_id))
        .push_bind(" AND bm.user_id = ", Id(user_id));
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

pub(crate) async fn get_members_for_board(
    scope: &mut Scope<'_>,
    board_id: &str,
) -> Result<Vec<BoardMember>, StoreError> {
    let mut sql = select_members(scope);
    sql.push_bind(" WHERE bm.board_id = ", Id(board_id))
        .push(" ORDER BY bm.user_id");
    fetch_members(scope, sql).await
}

pub(crate) async fn save_member(
    scope: &mut Scope<'_>,
    member: &BoardMember,
) -> Result<(), StoreError> {
    let mut sql = scope.sql(
        "INSERT INTO board_members (board_id, user_id, scheme_admin, scheme_editor, \
         scheme_commenter, scheme_viewer) VALUES (",
    );
    sql.bind(Id(&member.board_id))
        .push_bind(", ", Id(&member.user_id))
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
    Ok(())
}

/// Makes the transaction the only one that changes what is on the board, and who is
/// on it, until it ends; says whether the board exists.
///
/// What a write to a board decides on, it reads first: that a block's parent is there,
/// what is below a block that is deleted, whether there is another administrator. This
/// keeps what was read true until the write that relied on it. Every such write takes
/// the lock before it reads, so they queue up per board and read what the one before
/// them left.
///
/// On SQLite a transaction that writes holds the whole database from its start. On
/// PostgreSQL the board's row is locked, in the mode that leaves alone rows of other
/// tables that are stored for the board at the same time.
pub(crate) async fn lock_board(scope: &mut Scope<'_>, board_id: &str) -> Result<bool, StoreError> {
    Ok(lock_board_of_kind(scope, board_id).await?.is_some())
}

/// Locks a board as [`lock_board`] does, and says of what kind it is, which stays as
/// it is whatever is written; `None` if there is no such board.
pub(crate) async fn lock_board_of_kind(
    scope: &mut Scope<'_>,
    board_id: &str,
) -> Result<Option<BoardKind>, StoreError> {
    let mut sql = scope.sql("SELECT kind FROM boards WHERE id = ");
    sql.bind(Id(board_id));
    if scope.dialect() == Dialect::Postgres {
        sql.push(" FOR NO KEY UPDATE");
    }
    match scope.fetch_optional(sql).await? {
        Some(row) => Ok(Some(board_kinds::kind_from_text(&row.text(0)?)?)),
        None => Ok(None),
    }
}

/// Locks a board as [`lock_board`] does; one that is not there is not found.
pub(crate) async fn hold_board(scope: &mut Scope<'_>, board_id: &str) -> Result<(), StoreError> {
    if lock_board(scope, board_id).await? {
        return Ok(());
    }
    Err(StoreError::not_found(format_args!("board ID={board_id}")))
}

/// Holds a board that is about to be deleted against everything that is stored for it:
/// the lock the deletion of the row takes anyway, taken before the board is read.
///
/// [`lock_board`] leaves alone whoever stores a row that refers to the board (a
/// membership, a file record, a filing). A row stored between the reading of what goes
/// to the trash and the deletion would go with the board and not be in the trash. This
/// lock waits for such rows that are being stored and shuts out later ones, which then
/// find no board.
pub(crate) async fn lock_board_for_deletion(
    scope: &mut Scope<'_>,
    board_id: &str,
) -> Result<bool, StoreError> {
    let mut sql = scope.sql("SELECT id FROM boards WHERE id = ");
    sql.bind(Id(board_id));
    if scope.dialect() == Dialect::Postgres {
        sql.push(" FOR UPDATE");
    }
    Ok(scope.fetch_optional(sql).await?.is_some())
}

/// Takes the lock of [`lock_board`] on several boards, in the order of their
/// identifiers whatever order they are given in: two writes that span the same boards
/// then queue up instead of each holding what the other waits for. For a write that
/// touches more than one board, before it touches any.
pub(crate) async fn lock_boards<'a>(
    scope: &mut Scope<'_>,
    board_ids: impl IntoIterator<Item = &'a str>,
) -> Result<(), StoreError> {
    let ordered: BTreeSet<&str> = board_ids.into_iter().collect();
    for board_id in ordered {
        lock_board(scope, board_id).await?;
    }
    Ok(())
}

pub(crate) async fn delete_member(
    scope: &mut Scope<'_>,
    board_id: &str,
    user_id: &str,
) -> Result<(), StoreError> {
    let mut sql = scope.sql("DELETE FROM board_members WHERE board_id = ");
    sql.bind(Id(board_id))
        .push_bind(" AND user_id = ", Id(user_id));
    if scope.execute(sql).await? > 0
        && crate::board_team_access::role_for(scope, board_id, user_id)
            .await?
            .is_none()
    {
        // Whoever may no longer see the board is no longer told what changes on it.
        subscriptions::end_subscriptions_to_board(scope, board_id, user_id).await?;
    }
    Ok(())
}

pub(crate) async fn get_builtin_templates(scope: &mut Scope<'_>) -> Result<Vec<Board>, StoreError> {
    let mut sql = select_boards(scope);
    sql.push_bind(" WHERE is_builtin = ", true)
        .push(" ORDER BY create_at, id");
    fetch_boards(scope, sql).await
}

pub(crate) async fn remove_builtin_templates(scope: &mut Scope<'_>) -> Result<(), StoreError> {
    // The built-in templates are removed for good, past the trash.
    let mut sql = scope
        .sql("DELETE FROM blocks WHERE board_id IN (SELECT id FROM boards WHERE is_builtin = ");
    sql.bind(true).push(")");
    scope.execute(sql).await?;
    let mut sql = scope.sql("DELETE FROM boards WHERE is_builtin = ");
    sql.bind(true);
    let count = scope.execute(sql).await?;
    tracing::debug!(count, "removed the built-in templates");
    Ok(())
}
