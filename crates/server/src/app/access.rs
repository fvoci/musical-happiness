//! What is known of a user and a board: the facts [`permissions::grant`] is asked with.
//!
//! This and `Store::resolve_workspace` (by `App::actor`) are where the facts of a permission are read.
//! Every check of a route that names a board asks [`App::board_access`], and decides by
//! the [`Grant`] it gives; nothing else reads a membership to decide what a user may do.

use fvoci_model::{Board, BoardRole};
use fvoci_store::DeletedBoard;

use super::App;
use crate::error::AppError;
use crate::permissions::{self, Actor, BoardFacts, BoardScope, Grant};

/// A board, and what one user is and may do there.
#[derive(Debug, Clone)]
pub struct BoardAccess {
    /// The board as it is stored.
    pub board: Board,
    /// What the user is in the board's workspace and organization.
    pub actor: Actor,
    /// The user's role on the board as stored; `None` for who is no member.
    pub role: Option<BoardRole>,
    /// What the user may do on the board.
    pub grant: Grant,
}

/// A board in the trash, and what one user may do with it: what the role they had
/// when it was deleted would let them do, as what they are in its workspace now.
#[derive(Debug, Clone)]
pub struct DeletedBoardAccess {
    /// The board as the trash keeps it.
    pub deleted: DeletedBoard,
    /// What the user may do with the board.
    pub grant: Grant,
}

impl App {
    /// The board and what the user may do on it; `None` if there is no such board.
    /// A user without an identifier may do nothing.
    ///
    /// The same three reads are made whether the board is there or not, so that a
    /// board that is not there for the user takes as long to refuse as one that does
    /// not exist.
    ///
    /// # Errors
    ///
    /// Returns the store's error if a fact cannot be read: that is no "no", which would
    /// tell every member that their board does not exist.
    pub async fn board_access(
        &self,
        user_id: &str,
        board_id: &str,
    ) -> Result<Option<BoardAccess>, AppError> {
        let (board, member) = tokio::join!(
            self.store().get_board(board_id),
            self.store().get_member_for_board(board_id, user_id),
        );
        let board = match board {
            Ok(board) => Some(board),
            Err(error) if error.is_not_found() => None,
            Err(error) => {
                return Err(AppError::from(error).context("reading a board to check access"));
            }
        };
        let role = match member {
            Ok(member) => permissions::role_of(&member),
            Err(error) if error.is_not_found() => None,
            Err(error) => {
                return Err(AppError::from(error).context("reading the membership of a board"));
            }
        };
        let workspace_id = board.as_ref().map_or("", |board| &board.workspace_id);
        let actor = self.actor(user_id, workspace_id).await?;
        let Some(board) = board else {
            return Ok(None);
        };
        let grant = if user_id.is_empty() {
            Grant::NONE
        } else {
            permissions::grant(
                actor,
                BoardFacts {
                    scope: BoardScope::of(&board),
                    role,
                },
            )
        };
        Ok(Some(BoardAccess {
            board,
            actor,
            role,
            grant,
        }))
    }

    /// What the user may do on the board; nothing if there is no such board.
    ///
    /// # Errors
    ///
    /// Returns the store's error if a fact cannot be read.
    pub async fn board_grant(&self, user_id: &str, board_id: &str) -> Result<Grant, AppError> {
        Ok(self
            .board_access(user_id, board_id)
            .await?
            .map_or(Grant::NONE, |access| access.grant))
    }

    /// A board in the trash and what the user may do with it; `None` if no board of
    /// that identifier is there. The role is the one the trash kept of the user, and
    /// counts as what the user is in the board's workspace now: who has left the
    /// workspace since brings nothing back. A board in the trash is open to nobody
    /// (`BoardScope::in_the_trash`).
    ///
    /// # Errors
    ///
    /// Returns the store's error if a fact cannot be read.
    pub async fn deleted_board_access(
        &self,
        user_id: &str,
        board_id: &str,
    ) -> Result<Option<DeletedBoardAccess>, AppError> {
        let Some(deleted) = self.deleted_board(board_id).await? else {
            return Ok(None);
        };
        let actor = self.actor(user_id, &deleted.board.workspace_id).await?;
        let role = deleted
            .members
            .iter()
            .find(|member| member.user_id == user_id)
            .and_then(permissions::role_of);
        let grant = if user_id.is_empty() {
            Grant::NONE
        } else {
            permissions::grant(
                actor,
                BoardFacts {
                    scope: BoardScope::of(&deleted.board).in_the_trash(),
                    role,
                },
            )
        };
        Ok(Some(DeletedBoardAccess { deleted, grant }))
    }

    /// What the user is in the workspace and its organization, as
    /// `Store::resolve_workspace` answers: nothing for who is no member of it, and for
    /// a workspace that is not there or not in use.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the membership cannot be read.
    pub async fn actor(&self, user_id: &str, workspace_id: &str) -> Result<Actor, AppError> {
        let access = self
            .store()
            .resolve_workspace(user_id, workspace_id)
            .await?;
        Ok(access.map_or(Actor::default(), |access| Actor {
            org: access.org_role,
            workspace: Some(access.workspace_role),
        }))
    }
}
